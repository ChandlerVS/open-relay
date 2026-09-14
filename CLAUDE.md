# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Functional end-to-end for the core flow. Boot wiring (server, schema sync, OpenAPI, embed SDK, admin SPA) plus the domain resources — Users, Forms, Backends, Submissions — are implemented, along with auth/RBAC, OAuth provider config, secrets-at-rest, object storage for file-upload fields, and the delivery worker. Route handlers call into `crates/core` services; `NotImplemented` is just an `AppError` variant, not a stubbed handler. Still evolving: concrete delivery backends beyond the built-ins, more OAuth/SSO providers, and broader admin UX.

`OpenRelay.md` is the engineering design doc — it is gitignored, so consult it for intent but don't expect collaborators to have it.

## Stack & layout

Hybrid Cargo + pnpm/Turborepo monorepo.

- `apps/server/` — Axum HTTP API + delivery worker (Rust, edition 2024). Bin: `open-relay-server`.
- `crates/entity/` — SeaORM 2.0 entities. Hand-authored.
- `crates/core/` — Framework-agnostic domain logic (`Backend` trait, registry, delivery worker). Must not depend on Axum.
- `apps/admin/` — Vite + React 19 admin SPA (port 5173).
- `crates/mcp/` — MCP server (tools over `crates/core`). Like core, must not depend on Axum.
- `apps/mcp/` — stdio MCP binary. Bin: `open-relay-mcp`.
- `apps/embed-sdk/` — Vite library-mode IIFE bundle, dropped into host pages via `<script>`.
- `packages/api-client/` — OpenAPI-generated TS client (consumed by admin).
- `packages/form-renderer/` — Shared React form components (admin preview + embed SDK).
- `packages/ui/` — shadcn-style primitives (admin only).
- `infra/docker-compose.yml` — Local MySQL 8, plus a MinIO S3 store behind the `storage` profile.

## Commands

Prereqs: Rust (edition 2024), Node 22.11 (`nvm use`), pnpm 10, Docker.

```bash
# Local MySQL (required before server start)
docker compose -f infra/docker-compose.yml up -d mysql

# Optional: local S3 (MinIO) for developing file-upload fields
docker compose -f infra/docker-compose.yml --profile storage up -d

# Backend (binds 0.0.0.0:8080 by default; JSON API under /api/v1 e.g. /api/v1/healthz; /openapi.json, /docs at root)
cp .env.example .env   # first time only
cargo run -p open-relay-server

# Frontend
pnpm install
pnpm gen:api           # snapshots openapi.json → packages/api-client (server MUST be running)
pnpm gen:regions       # regenerates the ISO 3166 catalogues from data/iso-codes (offline)
pnpm gen:regions:check # asserts the checked-in catalogues are current
pnpm dev               # turbo: admin dev server + embed-sdk watch build
pnpm build             # turbo build, respects ^build ordering
pnpm typecheck         # turbo typecheck across all TS packages
pnpm lint              # most packages currently echo "no lint configured"
```

Single-package targeting: `pnpm --filter @open-relay/admin dev`, `cargo run -p open-relay-server`, `cargo test -p open-relay-core`, etc.

`gen:api` is two-stage (`scripts/fetch-openapi.mjs` then `openapi-typescript`). Override the source with `OPENAPI_URL=…`.

## Architecture notes that aren't obvious from the code

### SeaORM 2.0 entity-first — do NOT use `sea-orm-cli generate`

Schema is derived from Rust types and synced into MySQL at server boot via:

```rust
db.get_schema_registry("entity::*").sync(&db).await?;
```

This is idempotent and additive (creates missing tables/columns/keys, leaves the rest). When adding a new entity:

1. Create `crates/entity/src/<resource>.rs` following the pattern documented in `crates/entity/src/lib.rs`.
2. Add `pub mod <resource>;` to `crates/entity/src/lib.rs` — the `entity::*` glob auto-discovers it via the `entity-registry` feature. No central registration anywhere else.

### OpenAPI is generated from route attributes

Routes are mounted via `utoipa_axum::router::OpenApiRouter` + the `routes!` macro (see `apps/server/src/router.rs`, `routes/health.rs`, `auth/local.rs`). A handler only appears in `/openapi.json` if it carries a `#[utoipa::path(...)]` attribute and is passed to `routes!`. Tags declared on `ApiDoc` in `router.rs` must match the `tag = "..."` strings on handlers.

The TS client is regenerated from this spec; after adding/changing routes, restart the server and run `pnpm gen:api`.

### Form fields: `layout` is the source of truth, the legacy columns are a projection

A form's shape lives in three JSON columns on `form`, and the relationship between
them is the important part:

- `layout` (nullable) — an ordered `Vec<FormElement>` mixing standard fields, custom
  fields, headings/paragraphs/dividers, and `PageBreak` separators. This is what the
  builder edits and what current renderers consume.
- `standard_fields` + `custom_fields` — **derived from `layout` on every write.**

Both are kept because embed bundles cached on third-party host pages read the legacy
pair off `/api/v1/public/forms/{id}` and can never be force-upgraded. Rules:

1. **Never let the legacy columns go stale.** Every write path recomputes them via
   `legacy_from_layout`. If they ever went empty, `validate_and_split` would reject
   every submission and every cached bundle would render an empty form.
2. **`layout IS NULL` means "written before the column existed".** `layout_from_model`
   derives one from the legacy pair in the old renderer's exact order (enabled
   standards in `STANDARD_FIELD_KEYS` order, then customs by `position`). There is
   deliberately **no backfill** — the derivation is the migration.
3. **A request may send `layout` or the legacy pair, never both** (400). A legacy-only
   PATCH goes through `merge_legacy_into_layout`, which updates in place so a
   hand-ordered layout isn't flattened by a client that can't express order.
4. The projection is lossy in one direction only: decoration elements and a standard
   field's `placeholder`/`help_text`/`width`/`default_value` have no legacy home. That
   changes what an *old* bundle draws, never what the server validates or what a
   backend receives.

`FormElement` is **adjacently tagged** (`tag = "element", content = "config"`), not
internally tagged. Internal tagging round-trips fine here, but it and `serde(flatten)`
both forbid `deny_unknown_fields` — so a typo'd key in a layout save would persist as
an empty value. Adjacent tagging also keeps a custom element's `config` byte-identical
to the `CustomField` JSON already in `custom_fields`.

Multi-step forms are a flat list with `PageBreak` separators, not nested pages, so
stepping stays purely client-side and `submissions::service` needs no changes.

The standard-field set is generated by the `declare_standard_fields!` macro in
`crates/core/src/forms/mod.rs` — one list produces `STANDARD_FIELD_KEYS`, the
`StandardFieldsConfig` struct, and its key lookups. The `submission` entity columns and
`packages/form-renderer/src/standardFields.ts` still have to be updated by hand.

`crates/core/tests/layout_projection.rs` is the read-after-write guard for all of this.
It needs a live MySQL and is `#[ignore]`d by default:

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test layout_projection -- --ignored
```

### Post-submission action: the default is `NULL`, and the URL check is load-bearing

`form.post_submission_action` is a nullable JSON column holding a
`PostSubmissionAction` (`crates/core/src/forms/mod.rs`) — adjacently tagged like
`FormElement`, so `deny_unknown_fields` applies to every arm. Two things about it are
not obvious from the code:

1. **`NULL` is the default, and the default writes back as `NULL`.** Both create and
   update compare against `PostSubmissionAction::default()` and store `None` when they
   match, so "never configured" and "configured, then reverted" are the same row. That
   equivalence is what lets forms written before the column existed keep rendering the
   original thank-you copy with no backfill — the same no-backfill stance as `layout`.
2. **`validate_redirect_url` is a security boundary, not a tidy-up.** The embed SDK runs
   inline in third-party host pages (shadow DOM isolates CSS, not navigation), and the
   renderer hands this value straight to `window.location.assign`. Absolute `http(s)`
   only: a `javascript:`/`data:` scheme, a scheme-relative `//host`, or embedded
   whitespace (which browsers strip during URL parsing, letting `java\nscript:` re-form
   into a live scheme) is a hard 400. `packages/form-renderer/src/Form.tsx` re-checks the
   same rule before navigating, because the API response is untrusted input on a page we
   don't own.

There are two admin-facing choices for a message — with and without a "submit another"
button — but one wire variant; they differ only by `MessageAction::allow_resubmit`.
Message copy is plain text rendered with `white-space: pre-line`, never parsed as markup.

Preview surfaces must never navigate: the builder preview passes `previewMode`, and
`FormPreviewPage` (which submits for real) passes `suppressRedirect`. Both render a
"would redirect to …" panel instead.

`crates/core/tests/post_submission_action.rs` guards the round trip and is `#[ignore]`d
like the layout test:

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test post_submission_action -- --ignored
```

### Display names: the fallback resolves on the server, not in the renderer

A form has two names. `form.name` is the admin's own label — the forms list, the
select dropdowns, the builder header, the dashboard's `form_name` columns. The
nullable `form.display_name` is the heading a visitor sees. Two things about it
are not obvious from the code:

1. **The fallback is applied in `public_dto_from_model`, so `PublicFormDto.name`
   carries `display_name.unwrap_or(name)`.** The public DTO gained no field, and
   `packages/form-renderer` was not touched at all — `Form.tsx` still renders
   `schema.name` into the `<h2>` exactly as before. That is the entire point: an
   embed bundle cached on a third-party host page **honours a display name with
   no upgrade**, because it is reading the same key it always read. Adding a
   `display_name?` to `schema.ts` and coalescing in the renderer would be a
   regression, not an improvement — it would create a second copy of the
   fallback that only *new* bundles honour.

   The one place the admin has to repeat the fallback by hand is
   `FormBuilderPage`'s `previewSchema`, which assembles a `PublicFormDto`
   locally rather than fetching one. `FormPreviewPage` needs nothing: its
   `<ShadowForm>` fetches the public endpoint itself.

2. **`NULL` means "use `name`", and a blank string is how you get back to
   `NULL`.** Same no-backfill stance as `layout` and `post_submission_action`:
   never-configured and configured-then-cleared are the same row. `UpdateForm`
   carries a single `Option<String>` (absent = untouched) and runs it through
   `trimmed_within`, the idiom `UpdateRep`'s optional text fields already use —
   deliberately not `Option<Option<T>>`, which appears nowhere in the codebase.

`crates/core/tests/display_name.rs` guards the round trip and, in particular,
that the public read path and the admin read path disagree on purpose:

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test display_name -- --ignored
```

### Multi-step forms: `PageBreak` splits, `progress_indicator` decorates

A form is multi-step when its `layout` contains `FormElement::PageBreak`. The list
stays **flat** — breaks are separators, not nested pages — so stepping is purely
client-side and `submissions::service` is entirely page-blind. `validate_layout`
(`crates/core/src/forms/service.rs`) rejects only the degenerate positions: a leading
break, a trailing one, two in a row, and more than `MAX_PAGES` (20) pages.

`splitIntoPages` (`packages/form-renderer/src/layout.ts`) turns the layout into pages;
`Form.tsx` mounts **only the current page's fields**, which is what makes per-step
validation free — the browser's native constraint check can only see what's in the
DOM, so Next gates on this step alone. Next and Submit are both `type="submit"` for
that reason; Back is `type="button"` so it bypasses validation.

`form.progress_indicator` is a nullable JSON column holding a `ProgressIndicator`
(`{ style: bar | steps | none, show_percent }`). Two things about it are not obvious:

1. **`NULL` decodes to the bar, which is *not* the pre-column behaviour.** Forms
   written before the column drew a `Step N of M` line. This is a deliberate exception
   to the stance taken by `post_submission_action` above — the bar is the intended
   default presentation, and the property that actually matters (no backfill; a `NULL`
   always decodes to something valid) still holds. Both write paths still collapse the
   default back to `NULL`, so "never configured" and "reverted" remain the same row.
2. **An embed bundle cached on a third-party page can never honour this setting.** It
   predates the field, ignores it, and keeps drawing the step text forever. So `none`
   hides the indicator on *current* bundles only — never treat it as a guarantee that
   step counts aren't shown.

The percentage counts *completed* steps (`pageIndex / pages.length`), so step 1 of 4
reads 0% and the last step reads 75%; `aria-valuetext` carries the "Step N of M"
wording the bare number loses. A page break's `title` renders above the fields under
**every** style, including `none` — it names the step and has nowhere else to go.

`crates/core/tests/progress_indicator.rs` guards the round trip, `#[ignore]`d like the
others:

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test progress_indicator -- --ignored
```

### Conditional fields: rules point *backwards*, and the server prunes

Any field, heading or paragraph can carry `visible_when` — a `VisibilityRule`
(`crates/core/src/forms/mod.rs`) of one or more `Condition`s combined with
`match: all | any`. Four things about it are not obvious from the code:

1. **A condition may only name a field that appears strictly earlier in the
   layout.** `validate_layout` enforces it, and that one rule is doing three
   jobs: it rejects unknown keys, self-references and cycles at once, and it is
   what lets both evaluators resolve a whole form in a **single forward pass**
   with no fixpoint loop. Don't relax it without replacing it with a real cycle
   check.
2. **There are two implementations of one spec and they must agree exactly.**
   `crates/core/src/forms/visibility.rs` is the reference (its module docs are
   the spec); `packages/form-renderer/src/visibility.ts` is the copy that ships
   in the embed bundle. If the visitor's form and the server's validation
   disagree, someone gets a 400 they cannot act on. Two subtleties worth
   keeping: a condition naming a controller that is *itself* hidden evaluates to
   **false** — that is what makes hiding transitive *and* what stops a hidden
   field's prefilled `default_value` from steering a later element — and
   `is_checked` uses the same truthy vocabulary `coerce_custom` accepts
   (`true`/`on`/`yes`/`1`), with `is_not_checked` as its negation rather than
   "equals false", because an unanswered checkbox is absent, never `false`.
3. **Hidden values are dropped, not merely exempted from `required`.**
   `create_submission` computes the hidden set off the layout and hands it to
   `validate_and_split`, which removes those keys before any coercion. This is
   the reason the submission path stopped being layout-blind. It also means an
   embed bundle cached before rules existed — which renders and submits every
   field unconditionally — has its extra answers discarded rather than
   delivered. That is the intended reading ("the author said those fields don't
   apply"), but it is a real behaviour change for bundles that can never be
   force-upgraded.
4. **Legacy writes repair rather than reject.** A caller that speaks only
   `standard_fields`/`custom_fields` has no vocabulary for rules, yet its writes
   move elements: disabling a standard field removes it, enabling one inserts it
   at its catalogue position, a `position` reorder can move a controller behind
   its dependent. Any of those can strand a rule, so the legacy paths run
   `strip_dangling_rules` and the element simply becomes unconditional — 400ing
   a client over a rule it never sent and cannot see would be unactionable.
   Explicit `layout` writes skip this entirely: there the caller *did* send the
   rule, so `validate_layout` tells them what's wrong.

`FormElement::Divider` cannot carry a rule. It is a serde **unit** variant, and
under adjacent tagging giving it a body would require a `config` key that no
stored `{"element":"divider"}` has. The renderer's `visibleElements` collapses
dividers left leading, trailing or doubled once their neighbours hide, which is
the cosmetic half of the problem and all that actually mattered. `PageBreak`
can't carry one either, deliberately: pages stay unconditional so the step list
never churns on a keystroke. Instead `Form.tsx` **skips** a page with nothing
visible on it during Next/Back, and both the progress bar and the "Step N of M"
wording count only live steps so they can't disagree.

Hiding in the renderer is done by **unmounting**, never CSS. Per-step gating
relies on native constraint validation seeing only what is in the DOM, so an
unmounted field is exempt from `required` for free — whereas a `display:none`
required input makes the browser block submit on a control it refuses to focus.
`Form.tsx` also prunes hidden keys from the payload **subtractively**: rebuilding
it from the visible layout keys would silently drop the honeypot `_hp` and every
bot would sail through.

Rules on a *standard* element are dropped by `legacy_from_layout` like its
`placeholder`/`width`; rules on a *custom* field ride along in `custom_fields`,
because a `Custom` element's config is the `CustomField` JSON verbatim. Same
asymmetry `width` and `default_value` already have.

`CustomFieldType::Radio` was added alongside this — a `Select` with different
chrome, sharing its option rules and its submission coercion.

`crates/core/tests/conditional_fields.rs` guards the round trip, the pruning and
the legacy-repair behaviour, `#[ignore]`d like the others:

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test conditional_fields -- --ignored
```

### Country & state pickers: the catalogue is generated, the reference points *backwards*

Two custom field types — `country` (ISO 3166-1 alpha-2) and `state` (ISO 3166-2)
— let one form carry several address blocks. They exist as **custom** types
precisely because the standard `country`/`state` fields are singletons
(`declare_standard_fields!` makes one struct field each), so a billing *and* a
shipping pair is impossible with those. The standard pair also gained a select
variant (`input_override: "select"` now applies to `state`, not just `country`),
which covers the single-address case.

Five things aren't obvious from the code:

1. **The catalogue is generated into two checked-in files, not hand-written
   twice.** `scripts/gen-regions.mjs` reads the vendored Debian `iso-codes`
   data in `data/iso-codes/` and emits both
   `crates/core/src/forms/regions.json` (via `include_str!`) and
   `packages/form-renderer/src/regions.data.ts`. This is deliberately *not*
   the `visibility.rs` / `visibility.ts` situation: there are two copies, but
   one generator, so drift is a generator bug rather than a transcription one.
   `pnpm gen:regions:check` re-runs it and asserts `git diff --exit-code`.
   Output is checked in so neither `cargo build` nor the embed build needs
   node or the network.

2. **Only *top-level* subdivisions ship.** ISO 3166-2 entries carrying a
   `parent` are the nested tier — the UK's 200-odd districts under England /
   Scotland / Wales / NI, France's departments under its regions — and a
   "state / province" picker wants the top one. 3,590 rows rather than 5,046.
   One consequence to know: Ireland's top level is its four provinces, not its
   counties.

3. **The subdivision table rides on the form response, not the bundle.**
   Packed, it is ~40 KB gzipped against an embed script of 63 KB, and most
   forms have no state field — compiling it in would tax every host page for a
   table almost none of them use. So `PublicFormDto.regions` carries it, and
   only when `needs_subdivisions(&layout)` says the form has a field that
   draws one. The country list *is* in the bundle (~2 KB gzipped): the country
   picker is the entry point and must render synchronously. Net effect on
   `open-relay.js`: 63.2 → 65.6 KB gzipped, and `PACKED_SUBDIVISIONS` tree-shakes
   out of it entirely. The admin preview imports the table directly, having no
   such budget.

4. **`country_field` points strictly backwards, and that one rule does three
   jobs** — exactly like `visible_when`. `validate_layout` carries a single
   `SeenField { is_checkbox, is_country }` map through one forward pass, so
   unknown keys, self-references and cycles all fall out of the same check, and
   both evaluators resolve a whole form without a fixpoint loop. A state picker
   may name a `Country` custom field, or the standard `country` element **when
   it is a select** — a plain-text country holds a *name*, not a code. Note
   `CustomFieldType::options()` deliberately returns `None` for both new
   variants even though they render dropdowns: their choices aren't
   author-declared, and reporting them would make the "offers a choice but has
   no options" rule nonsense and hand the admin's rule editor a 3,590-entry
   operand dropdown.

5. **Legacy writes repair rather than reject, and unbound means free text.**
   `strip_dangling_country_refs` runs beside `strip_dangling_rules` on the two
   legacy write paths only. A legacy client can disable the standard `country`
   field or reorder a country picker behind its dependent, stranding a
   reference it has no vocabulary for; 400ing over that would be unactionable.
   So the state field becomes unbound — which renders and validates as free
   text, exactly what the standard `state` field has always done. The same
   fallback covers a country with no ISO subdivisions (49 of 249) and a bundle
   whose server never sent a table, so the renderer and `coerce_custom` agree
   on every one of those cases.

A subdivision is stored as the **bare** code (`CA`), not the full ISO 3166-2
code (`US-CA`): the country is captured by the sibling field, and
`backend::gohighlevel` forwards `state` verbatim, so `CA` is what reaches the
CRM. `coerce_custom` gets the country from the answers already coerced ahead of
it — sound with no second pass, because `country_field` must be earlier and
`custom_fields` is in layout order. A country that is unanswered **or hidden**
reads the same way (a hidden field's answer is dropped before coercion), so a
non-empty state with no country is a 400 on both sides.

Deliberate non-change: the *standard* `state`/`country` fields get no
server-side membership validation. `input_override` is layout-only and is
dropped by `legacy_from_layout`, so gating on it would 400 a legacy author over
something they cannot see — and the existing standard country select already
worked this way. For standard fields the select is presentation only.

`stateBindings` (`layout.ts`) is the renderer's single source of truth for the
binding, both directions: which country's list a state draws, and which states
to clear when a country changes — so `CA` can never be submitted under `FR`, a
pairing the server rejects on a field that would look answered. It mirrors what
`validate_layout` accepts, including the standard pair needing *both* halves to
be dropdowns, so the builder preview can't show a working dropdown for a form
that won't save.
Country lookups read the **defaults-merged** value map, not raw state, for the
same reason the visibility memo does: the prefill is an effect, so a country
with a `default_value` would otherwise draw its state as a text box for one
frame. `CustomFieldInput` now ends in a `never` exhaustiveness guard — the
fallback `<input type={field.type}>` used to swallow an unknown variant
silently.

`crates/core/tests/region_fields.rs` guards the round trip, the cross-field
coercion and the legacy repair, `#[ignore]`d like the others:

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test region_fields -- --ignored
```

### Row blocks: a flat marker pair, and width is read two ways

Several fields sit on one line by putting them between a `FormElement::RowStart`
and a `FormElement::RowEnd` (city / state / postal code being the case it was
built for). Four things aren't obvious from the code:

1. **A row is a flat marker pair, not a container with `children`.** The whole
   layout system rests on one flat slot per element: `element_visibility` returns
   a `Vec<bool>` *positionally parallel* to the layout, `FormPage.offset` indexes
   into that by `offset + i`, `coerce_custom` reads earlier answers out of a
   `custom_fields` list that is in layout order, and `validate_layout` resolves
   rules and country references in one forward pass. Nesting breaks all of those
   at once; markers break none of them, because they register no key and every
   pass walks straight past. This is the same call `PageBreak` made — pages are a
   grouping computed from separators, not a nested structure — and it is what
   keeps the builder a **single flat `SortableContext` with
   `restrictToVerticalAxis`**: dragging a field into a row is an ordinary
   vertical reorder, not a cross-container drop.

   It is also what keeps the projection honest. Fields in a row are still
   top-level elements, so `legacy_from_layout` still writes them to the legacy
   pair and an embed bundle cached before rows existed still renders them —
   stacked, since it ignores the markers. Same way `width` already degrades
   there. `crates/core/tests/layout_rows.rs` pins that specifically.

2. **`FieldWidth` is read two ways against one denominator of six.** Outside a
   row it is a span of the six-track grid (`half` = 3 tracks, `third` = 2);
   inside a row it is a *flex weight*. `Full` weighs six, so a row whose fields
   all leave the width alone splits evenly — which is what you want when three
   fields were put in a row precisely so they'd sit side by side. The grid went
   2 → 6 tracks to make thirds expressible; `half` was `span 1` of 2 and is now
   `span 3` of 6, i.e. the same 50%, so no existing form's rendering changed.

3. **A row carries no `visible_when`, deliberately.** Its fields keep their own
   rules, and `visibleElements` collapses a row whose fields have all hidden —
   the same pass that already collapses a stranded divider. Doing it *there*
   rather than at render time is load-bearing: `Form.tsx` decides which steps are
   live by asking whether a page has any visible elements, so an all-hidden row
   has to come back empty or its page counts as a step with nothing on it. Only
   `standard`/`custom` may sit in a row (`FormElement::allowed_in_row`); a page
   break inside one would split a line across two steps.

4. **The builder repairs rather than restricts.** A flat list lets a drag invert
   or strand a marker, so `Canvas`'s `onDragEnd` runs `normalizeRows` after the
   `arrayMove` — it un-nests, closes, and drops empty rows so a reorder can never
   produce a layout that won't save. Deleting either marker takes its partner
   (`removeMany` pairs them, and `normalizeRows` repairs the rest), since half a
   pair is a 400. The server mirrors the split:
   `normalize_layout` drops an *empty* row quietly (cosmetic debris, like a rule
   with no conditions), while `validate_layout` rejects structural breakage. That
   quiet cleanup is also why the legacy write paths need no repair pass of their
   own — `merge_legacy_into_layout` can empty a row by disabling its fields, and
   both paths already call `normalize_layout`. The one thing that pass *did* need
   is a guard so a newly enabled standard field is never inserted **inside** a
   row (`row_start_before` / `row_end_after`): a legacy client can't see a row and
   never asked for one.

`FormElement` now has 8 variants, so a new one surfaces first at the exhaustive
`shape()` match in `crates/core/tests/layout_projection.rs`. `RowStartElement` is
a newtype variant carrying an optional `label` (rendered as `role="group"`), not a
unit variant — same lesson `Divider` records, that a unit variant can never grow a
body under adjacent tagging.

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test layout_rows -- --ignored
```

### Rich text: markdown in, React out — never HTML

`FormElement::RichText` holds **markdown source** in a `markdown: String`. It is
the block that carries markup; `Paragraph` stays what it always was, a single
escaped text node, and is not going away (every stored layout may contain one,
and `deny_unknown_fields` means it must keep deserialising). Five things aren't
obvious from the code:

1. **The renderer builds React elements, so HTML injection is structurally
   impossible and the attack surface reduces to one attribute.**
   `packages/form-renderer/src/markdown.ts` parses to an AST of plain objects
   and `RichText.tsx` maps that to JSX, so every leaf is a React text child that
   React escapes — `<script>x</script>` in a block renders as those characters.
   That is what lets the embed skip a sanitiser it cannot afford: `marked` plus
   `DOMPurify` is 30 KB+ against a bundle of 66 KB, while the whole parser,
   component and CSS came to **2.0 KB gzipped** (measured, 67,545 → 69,637 B).
   Nobody may reach for `dangerouslySetInnerHTML` to simplify a preview; that
   one change reinstates every class of injection this design assumes away.

2. **There are two link checks and they point in opposite directions on
   purpose.** `service::validate_markdown_links` finds every `](…)` in the
   source — including inside a code span, inside `![]()`, inside text the
   renderer draws literally — and runs each through `validate_http_url`, the
   same function `validate_redirect_url` uses. The renderer checks each
   destination with `isHttpUrl` (`packages/form-renderer/src/url.ts`, hoisted
   out of `Form.tsx` now that it has two callers) and renders plain text rather
   than an `<a>` when it fails. The invariant is containment —
   **flagged-by-server ⊇ linked-by-renderer** — so drift can only cost an author
   a save error naming the exact string, never produce a live `javascript:`
   href.

   This is deliberately *not* the `visibility.rs`/`visibility.ts` situation.
   There both copies must agree exactly, because either direction of
   disagreement is a bug. Here only one direction is a bug, and the safe
   direction is the cheap one to guarantee. What holds it up is that the
   renderer's link grammar is **frozen at one production**, `[text](dest)` — no
   reference links, no autolinks, no angle-bracket destinations, no titles.
   Adding any of those to the parser breaks containment silently, because the
   scanner would keep passing markdown the renderer had newly started linking.
   Freeze the grammar; don't chase it with a broader scanner.

3. **Unsupported syntax renders literally; nothing is ever dropped.** A pasted
   table, an image, a blockquote, a raw tag — all drawn as the characters the
   author typed. A block that renders half of what was typed is a worse failure
   than one that renders it verbatim, and `markdown.test.ts` machine-checks the
   property: every alphanumeric run in the source must survive into some text
   node. That test is also why an empty emphasis span (`**` inside `*…*`) is
   refused rather than matched — matching it would consume the delimiters and
   emit nothing.

4. **Tone is a named role, never a colour, and the default stays off the wire.**
   `RichTextTone` (`normal | muted | info | warning | danger`) resolves to theme
   tokens, because the embed draws on a host page whose palette we don't
   control; there are deliberately no inline colour spans. `Normal` is skipped
   on serialisation (`RichTextTone::is_default`), the same stance
   `post_submission_action` takes — and here it has a second, concrete
   consequence: the builder's dirty check is a `JSON.stringify` comparison
   against what the server sent, so the admin's `withTone` must **delete** the
   key on `normal` rather than write it, or every form with a block reads as
   edited on load. The three loud tones tint their own background rather than
   only colouring text, because `--or-bg` defaults to `transparent` and coloured
   text on an unknown host background has unknowable contrast.

5. **An old cached bundle draws nothing where the block is.** A bundle that
   knows `layout` falls through the render switch's `never` guard to `null`; one
   predating `layout` never sees it, since `legacy_from_layout` drops it like
   every other decoration. Both mean *invisible* — a harder degradation than a
   row (which stacks) or a `width` (which goes full-width). So **don't put
   legally load-bearing copy in a rich-text block** until host pages have cycled
   their bundles: on an old one the visitor never sees the notice and submits
   anyway.

`normalize_layout` folds CRLF and trims the ends only — interior blank lines are
paragraph breaks and leading spaces are list indentation, so anything more
aggressive would rewrite the author's document. The block is excluded from rows
by `allowed_in_row` for free, being a flow of block-level content.

The parser is the first genuinely algorithmic thing in the TS half with no Rust
twin behind it, so it is also the repo's first JS test suite: `node --test` with
type stripping, **no new dependency** (`@types/node` is dev-only).

```bash
pnpm --filter @open-relay/form-renderer test

DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test rich_text -- --ignored
```

### File uploads: the value is a sealed receipt, not a URL

`CustomFieldType::File` collects one file per field. Bytes never touch this
server — the browser gets a presigned `PUT` and uploads straight to the
configured object store — and what lands in `custom_data` is the object's URL,
so `delivery_data` and every backend forward it verbatim with no special-casing.

Storage is an abstraction (`crates/core/src/storage/`) mirroring
`crates/core/src/backend/` exactly: a `FileStore` trait, a `FileStoreFactory`
whose `secret_keys()` drives DTO redaction / update-preservation / encryption
at rest, and a `StorageRegistry` registered in `AppState::new`. S3 (and any
S3-compatible store) is the only kind today. Config is a **single active row**
in `storage_provider`, edited at Settings → File storage under a new
`storage_config:write` permission — the `oauth_provider_config` shape, not the
`backend_instance` one, because storage is deployment-wide.

Six things aren't obvious from the code:

1. **The submitted value is a sealed receipt, and that is the whole security
   model.** `POST /public/forms/{id}/uploads` has to be unauthenticated (it
   serves embedded forms on third-party pages), so whatever comes back at
   submit time is attacker-controlled. If the field value were a URL, anyone
   could POST any URL and have it delivered into a CRM record. Instead the
   presign handler seals `v1|{form_id}|{field_key}|{issued_at}|{object_key}`
   with the **existing** `SecretCipher` (`storage/receipt.rs`) — XChaCha20-
   Poly1305 is an AEAD, so this is a signature with no new key material, no new
   dependency and no new table. `resolve_file_uploads` opens it and swaps in
   `store.stored_url(...)` **before** `validate_and_split` runs; the `File` arm
   of `coerce_custom` reads that map rather than the raw string, so if the
   pre-pass is ever skipped every file field 400s instead of quietly accepting
   whatever arrived. Don't "simplify" that arm to trust its input.

2. **`Content-Length` and `Content-Type` are signed into the presigned URL, and
   that is the size limit.** The server-side check on the declared `size` only
   produces the friendly error; the signature is the enforcement, because a
   browser sets `Content-Length` from the body and can't be told to lie.
   `crates/core/tests/file_uploads.rs` proves this against a live MinIO — an
   oversized body really is rejected.

3. **SigV4 is hand-rolled (`storage/s3.rs`), deliberately.** `hmac`, `sha2`,
   `base64` and `chrono` were already workspace deps and the only operations
   needed are query-string presigning of PUT/GET; `aws-sdk-s3` would pull the
   whole smithy tree for ~150 lines. Same call the markdown parser made. The
   canonical request is pinned against AWS's published worked example
   (`canonical_request_matches_published_aws_example`) because canonicalisation
   is the half that breaks silently — the only runtime symptom is an opaque
   `SignatureDoesNotMatch`. Run the MinIO test after touching signing.

4. **`visibility: public` means a bucket policy, never an ACL.** No `x-amz-acl`
   header is signed: buckets created since April 2023 default to Object
   Ownership "bucket owner enforced" and reject request ACLs outright, and
   R2/MinIO/B2 each diverge again. The admin page prints the policy to paste.
   `presigned` keeps the bucket private and stores an expiring GET (SigV4 caps
   at 7 days), at the cost of the link dying inside a delivered CRM record.

5. **There is deliberately no `uploaded_file` table, so orphans are a lifecycle
   rule.** A presigned-but-never-submitted object is indistinguishable from a
   live one without persistence, and adding a row per ticket would turn an
   unauthenticated endpoint into an unauthenticated `INSERT`. The admin page
   says to set an object-expiry rule on the prefix. A receipt is also *reusable*
   within its 24h TTL — the same file can be attached to two submissions of the
   same form. Both are accepted trades, not oversights.

6. **An embed bundle cached before this change draws nothing where a file field
   is.** A bundle that knows `layout` falls through the render switch's `never`
   guard to `null`; one predating `layout` never sees it, since
   `legacy_from_layout` drops nothing here — a file field *is* a custom field,
   so it rides in the legacy pair, but an old renderer has no `file` case. This
   is the harshest degradation in the codebase: worse than a row stacking or a
   `width` going full-width, because a **required** file field on an old bundle
   makes the form uncompletable. Don't add one to a widely-embedded form until
   host pages have cycled their bundles.

The renderer keeps `values` as `Record<string, string | boolean>` — the value
is the receipt string — and holds upload progress in a *separate* `uploads`
map. That is what leaves `visibility.ts`, `layout.ts`, `hiddenKeys` and the
subtractive payload build untouched. A hidden file field's answer is dropped
before it is ever resolved, so a conditional upload mints no URL.

`packages/form-renderer/src/uploads.ts`'s `localRejection` mirrors the server's
`accept_matches`, but only as a courtesy: the server re-validates everything, so
drift is a UX bug, never a hole. This is deliberately *not* the
`visibility.rs`/`visibility.ts` situation.

Local development gets a real S3 via the compose `storage` profile:

```bash
docker compose -f infra/docker-compose.yml --profile storage up -d
# creates the open-relay-uploads bucket, public-read, ready for the admin page

DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test file_uploads -- --ignored
```

### Rating fields: an integer on the wire, radios in the DOM

`CustomFieldType::Rating { max }` is a star rating of 1..=`max` stars, where `max` is
3–10 and defaults to 5 (`MIN_RATING_MAX`/`MAX_RATING_MAX`/`DEFAULT_RATING_MAX`,
mirrored in the builder's `model.ts`). Three things aren't obvious:

1. **The stored answer is a JSON integer, and `coerce_custom` refuses floats
   outright.** The renderer holds the answer as the string `"7"`, and a
   visibility rule compares canonical strings. `Number(7)` canonicalises as
   `"7"`, but a float would come out as `"7.0"` and silently stop matching
   `equals 7` on the server while it still matched in the browser.
   That is the `visibility.rs`/`visibility.ts` disagreement in miniature.
   Like `Country`, `options()` reports nothing. The builder offers `1..max` as
   rule operands through `ratingOptions` instead.
2. **It is a radio group with the buttons hidden behind the stars.** That gives
   native `required`, so per-step gating sees it, plus arrow-key navigation
   and accessible names with no extra code. The inputs are hidden with
   opacity, **never** `display: none`: a hidden required radio blocks submit
   on a control the browser refuses to focus. `RatingInput` is its own
   component because it keeps hover state, and `CustomFieldInput` returns
   early before any hook could run.
3. **Old cached bundles degrade like `file` does.** A bundle that knows `layout`
   hits the `never` guard and draws nothing, so a **required** rating makes the
   form uncompletable there. A bundle predating `layout` falls back to
   `<input type="rating">`, a text box, and the server still coerces a typed
   `4`.

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test rating_fields -- --ignored
```

### Themes: a third variable tier, and the host page still wins

A `theme` row (`crates/core/src/themes/`) is a reusable look — colours, corner
radius, font family, text size, spacing density — and `form.theme_id` names
one. Admins manage them at `/themes` (`themes:read|write|delete`). Five things
aren't obvious from the code:

1. **The renderer reads a theme *beneath* the public tokens, never over
   them.** Every engine token in `styles.css` is
   `var(--or-color-accent, var(--or-theme-accent, #111827))`, and
   `themeStyle` (`packages/form-renderer/src/theme.ts`) sets only the
   `--or-theme-*` tier, inline on the form root. The obvious alternative —
   setting `--or-color-*` inline — would beat the host page's `:root` rules,
   so every embed a customer had already themed by hand would silently change
   the moment an admin picked a theme. Radius, font, and the two new scales
   (`--or-font-scale`, `--or-space-scale`, now public tokens too) take the
   same host → theme → built-in chain via `--or-r` / `--or-fs` / `--or-sp`.
   The scales multiply the hard-coded rem sizes, so `1` renders exactly what
   shipped before.

2. **"Default" is resolved on the server.** `resolve_settings` walks own
   theme → the `is_default` row (`ORDER BY id`, so a race that flagged two
   still reads deterministically) → `None`, and `get_public_form` puts the
   result on `PublicFormDto.theme` — the `uploads_enabled` post-projection
   step, since it needs a read. The renderer never learns what "default"
   means, and neither does the builder: its preview reads `.theme` off the
   public endpoint (`useResolvedFormTheme`) rather than repeating the fallback
   — the opposite of the `display_name` situation, where the admin DTO is
   enough to rebuild it. One default is kept by the service (`clear_default`
   inside the write's transaction); MySQL can't express "unique among `true`".

3. **Every member is optional, and one palette covers both modes.** An unset
   colour falls through to the built-in value for the embed's `data-theme`, so
   a theme that sets only `accent` still looks right on a dark embed; a theme
   that sets `text` does not adapt. Defaults are skipped on serialisation (the
   empty theme is `{}`), and the editor's `canonicalSettings` rebuilds objects
   in server key order — its dirty check is a `JSON.stringify` comparison.
   A stored row that fails to parse reads as `{}` with a warning: a theme
   must never be able to 500 a public form.

4. **Validation is a CSS-injection boundary on both sides, containment-style.**
   Colours are hex only, a font family is `[A-Za-z0-9 ,'"_-]` with balanced
   quotes, radius is 0–32. That excludes `url()` (a request from someone
   else's page), `var()` and `;`. `themes::service::validate_settings` 400s;
   `themeStyle` re-checks and *drops*. Like rich-text links, only one
   direction of drift is a bug and the server's set is never the wider one.

5. **`theme_id: 0` is how an update clears it**, the id-shaped version of a
   blank `display_name` (ids start at 1) — deliberately not `Option<Option<_>>`.
   Deleting a theme sets its forms back to `NULL` in application code (the
   `reps::service::delete` pattern; there are no DB foreign keys), so they
   fall back to the default rather than blocking the delete. An embed bundle
   cached before themes ignores `theme` and draws the built-in look — the
   gentlest degradation in this file.

```bash
pnpm --filter @open-relay/form-renderer test

DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test themes -- --ignored
```

### Builder clipboard: the paste is repaired on the client, because a `layout` write isn't

The builder canvas is multi-select (shift-click for a range, Cmd/Ctrl-click to
toggle) and a selected block can be copied, cut, duplicated and pasted — including
into a **different form's** builder, which is the case it was built for: an address
block is a dozen elements of country/state bindings and visibility rules that
nobody wants to retype. Transport is a `text/plain` JSON envelope on the system
clipboard (`builder/clipboard.ts`), mirrored into `localStorage`. Four things
aren't obvious:

1. **The repair has to happen at paste time, because the server won't do it.**
   `strip_dangling_rules` and `strip_dangling_country_refs` run only on the legacy
   write paths — the reasoning being that a caller who sent a `layout` sent the
   rules too and deserves the 400. A pasted block is the one case that breaks: the
   rules came from another form and the author never typed them here. So
   `prepareForPaste` (`builder/blockOps.ts`) does the server's repair itself, in a
   forward pass whose key→is-checkbox and country-key maps are built exactly the
   way `validate.ts` builds them — agreeing with the inline validation *is* the
   requirement. `crates/core/tests/paste_roundtrip.rs` pins the output against the
   real `validate_layout`, which is the half TypeScript can't check.

2. **Re-keying has to repoint the block's own references, not just the keys.**
   Pasting a block twice into one form renames `billing_city` → `billing_city_2`,
   and the copy's state picker must bind to `billing_country_2`, not to the
   original's. `taken` is seeded with the block's own keys as well as the
   target's, so a generated name can never collide with a key still to be
   processed — which would make the rename map ambiguous and repoint the wrong
   element. A reference whose target stayed behind is dropped (condition removed,
   rule removed with its last condition, `country_field` **deleted not nulled** —
   the dirty check is a `JSON.stringify` comparison).

3. **`selectedId` is derived, which is why `Inspector.tsx` was not touched.**
   `selection.ids.length === 1 ? ids[0] : null` — so every existing
   `selectedIndex` consumer works unchanged, and a multi-selection falls to the
   Inspector's existing "nothing selected" branch. The selection is **pruned
   against the live list on every read** rather than trimmed on write, because
   `normalizeRows` both drops ids and *mints* one for a closer it synthesizes.
   Row markers are deliberately **not** expanded to include their contents:
   selecting a `row_start` has to leave one element selected or the row's label
   editor becomes unreachable. Pairing happens in `removeMany` instead, which is
   what makes the trash button and a bulk delete one code path.

4. **Firefox and Safari never fire `paste` here, so the toolbar button is load-
   bearing.** Both gate the event on an editable focus context and a canvas card
   is a `<button>`; `navigator.clipboard.readText()` is no fallback either (not
   exposed to page content in Firefox, gesture-plus-prompt in Safari, absent in a
   non-secure context). Hence three rungs: the async API, the `localStorage`
   mirror, then a textarea to paste into by hand. Every guard reads
   `e.composedPath()[0]`, not `e.target` — the live preview is a **shadow root**,
   and an event from inside one is retargeted at the host by the time it reaches
   `document`, so `e.target` would show a `<div>` where the user is typing in an
   `<input>` and Cmd+A in the preview would select the whole canvas.

Positions are renumbered on every block write (`renumberPositions`), mirroring
`normalize_layout`. Without it a pasted block carries the source form's
`position` values, the dirty check never re-converges with what the server sends
back, and the form reads as edited forever after a successful save.

### Submissions list: one filter builder, and CSV cells are defused

The admin list (`GET /submissions`) and the CSV export (`GET /submissions/export`)
take the same filters — `q`, `form_id`, `status`, `sales_rep_id`, `duplicate`,
`from`, `to`, `sort` — and four things aren't obvious from the code:

1. **Both lower into one `SubmissionFilter` and one `filtered_select`.** The query
   structs repeat the filter fields rather than sharing them through
   `serde(flatten)`, because flatten buffers query values as strings and breaks
   every numeric field under `serde_urlencoded`. Duplicating the *declarations* is
   fine; duplicating the *query* is not — an export must return exactly the rows
   the table showed. The export pages by keyset, not offset, so a submission
   arriving mid-export can't duplicate a row.
2. **Search is AND-of-terms, OR-of-columns.** Each whitespace term must match one
   of the typed columns, `LOWER(CAST(custom_data AS CHAR))`, or (numeric, `#` optional)
   the id — which is how "jane acme" finds Jane at Acme. `escape_like` is load-bearing:
   without it `50%` matches `500 off`. The `LOWER` is too: a JSON cast carries a
   binary collation, so it is case-sensitive where the typed columns aren't.
   Every search is a full scan; fine at admin scale, and the reason for the caps
   (`MAX_SEARCH_LEN`, `MAX_SEARCH_TERMS`).
3. **`status` means "has *any* delivery in the set"**, so a submission that failed
   on one backend and succeeded on another shows under both Failed and Delivered.
   `to` is exclusive and the admin computes both bounds from local calendar dates
   in the *browser's* timezone (`lib/submissions/filters.ts`) — the server only
   ever sees instants.
4. **`csv_field`'s formula guard is a security boundary.** Submission values are
   written by anonymous visitors and the file is opened in a spreadsheet, so a
   cell starting `=`, `+`, `-`, `@`, tab or CR gets a leading `'`. That means a
   phone number like `+1 555…` exports as `'+1 555…`; don't narrow the guard to
   "looks like a formula". The export is built in memory and refused past
   `MAX_EXPORT_ROWS` (50 000) with a 400 telling the admin to narrow the filters.

The admin keeps every piece of view state in the URL, including the open detail
sheet (`?submission=ID`), so a filtered view survives a reload and can be shared.
`form_id` predates the rest and is what the dashboard links to — keep the name.

```bash
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test submission_filters -- --ignored
```

### GoHighLevel custom fields are matched by id, not by the key you configured

The admin sets an OpenRelay custom field's `key` to name the GHL field it
should land in, and `delivery_data` hands that key through verbatim. What goes
on the wire is *not* that key. Three things about it aren't obvious:

1. **GHL resolves a `customFields` entry by `id`.** `id` is the only required
   member of the entry schema, and an entry it can't resolve is answered with a
   plain `200` and no stored value — the failure is completely silent, which is
   why a misconfigured key looks like a working delivery with empty fields. So
   `GoHighLevelBackend` reads the location's catalog
   (`GET /locations/{id}/customFields?model=contact`) and sends
   `{ id, key, field_value }`.
2. **The `key` GHL wants is the *bare* key, never the `contact.`-prefixed form
   its own UI prints.** `normalize_field_key` lower-cases, strips `{{…}}` and
   strips the model prefix, so `{{contact.billing_city}}`, `contact.billing_city`
   and `billing_city` are one field. The catalog index falls back to the display
   name, but `fieldKey` always wins — two fields can share a name.
3. **The catalog read degrades rather than fails.** A PIT without the
   `locations.readonly` scope is the common case; losing the whole contact over
   an unresolvable *custom* field would be worse than delivering it key-only, so
   a failed read logs a warning and yields an empty index (deliberately
   *uncached*, so a scope fix takes effect on the next delivery rather than in
   five minutes). Successful reads are cached per location for
   `FIELD_CACHE_TTL` on the **factory** — `BackendFactory::build` runs once per
   delivery, so a cache on the backend would never be hit.

Note `field_value` (snake) is correct despite the docs *site* rendering
`fieldValue`; GHL's published OpenAPI for `/contacts/upsert` says `field_value`.

### Backend delivery is a registry of trait objects

`open_relay_core::backend::Backend` is the integration surface (GoHighLevel, OpenRelay's own store, etc.). Implementations register against the `BackendRegistry` held in `AppState`, constructed in `AppState::new` (`apps/server/src/state.rs`) — it registers `OpenRelayBackend` (static) and `GoHighLevelFactory` at boot today. New backends register there: `register_static` for config-less backends, `register_factory` for ones built per `backend_instance` row.

Secret-bearing keys inside a `config` JSON column — for backends and for
storage alike — are handled by `crates/core/src/secrets.rs`, which owns the
`preserve → decrypt → validate → encrypt` ordering both write paths depend on.

`DeliveryError` distinguishes `Transient` (worker retries) from `Permanent` (no retry, admin notify). `Backend::deliver` must be idempotent on `submission_id`.

### Delivery worker

`crates/core/src/jobs/worker.rs` spawns a tokio loop that leases due `submission_delivery` rows with `SELECT … FOR UPDATE SKIP LOCKED`, dispatches each to its `Backend`, and records the outcome. Transient failures are retried on an exponential backoff (30s → 24h over `MAX_ATTEMPTS` = 6) then marked exhausted; permanent failures are not retried. Stale `in_progress` leases (worker crash mid-delivery) are reclaimed on startup. `Backend::deliver` must be idempotent on `submission_id`.

### Auth is local JWT + pluggable Provider trait

- `crates/core/src/auth/` — `AuthKeys`, `Claims`, JWT issue/verify, and the `Provider` trait + `ProviderRegistry` for OAuth/SSO. Framework-agnostic. The `oauth2` crate is in workspace deps; no concrete providers ship in the skeleton.
- `apps/server/src/auth/local.rs` — `POST /auth/login`, calls into `open_relay_core::users::service` + `open_relay_core::auth::issue_for_user`.
- `apps/server/src/auth/mod.rs` — Axum-only bits: `AuthUser` extractor that calls `core::auth::verify_jwt` against `AppState::auth_keys`.

### Embed SDK isolates via Shadow DOM

`apps/embed-sdk` builds to a single IIFE (`open-relay.js`) with React/ReactDOM bundled in (no peer-dep on the host page). At runtime it reads `data-form-id`/`data-api-url` off the executing `<script>`, inserts a sibling `<div>`, attaches an open shadow root, and applies its CSS via a constructable stylesheet so the host page's styles can't bleed in.

### Tailwind v4

The admin uses Tailwind v4 via `@tailwindcss/vite` (no `tailwind.config.js` — config is CSS-driven). The embed SDK uses plain CSS imported with `?inline` for shadow-root injection.

### TypeScript

All TS packages extend `tsconfig.base.json` (strict, `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, `noEmit`). Build is via Vite or tsgo (TS 6); `pnpm typecheck` runs `tsc --noEmit` everywhere.

### MCP: an agent-facing surface over the same services, on a third credential

`crates/mcp` exposes forms to LLM agents over the Model Context Protocol
(`rmcp` 3.3). It is to an agent what `apps/admin` is to a person, and it is
wired the same way: tools call `open_relay_core::forms::service` directly, so
every invariant above — `validate_layout`, `normalize_layout`, the legacy
projection, the backwards-pointing rules — applies unchanged. Nothing in
`crates/mcp` validates a layout; a tool that did would be a second copy of a
spec with one owner.

Six things aren't obvious from the code:

1. **Agents authenticate with an API key, not the access JWT, and that is a
   UX constraint rather than a security one.** `ACCESS_TTL_SECONDS` is 15
   minutes and the refresh token *rotates on every use* — an MCP client holds a
   static string in a config file and has nowhere to write a rotated secret
   back to. So `entity::api_key` is a third credential: opaque, long-lived,
   revocable, hashed at rest. It copies `auth/refresh.rs` (same
   `generate_secret`/`hash_secret`, same "every failure is `Unauthorized`" rule
   so there is no existence oracle) and deliberately does **not** rotate.

   A key's permissions are `owner's live permissions ∩ key scopes`, resolved on
   **every** call. So a key can never outrank its owner — which is why issuing
   one needs no permission of its own and lives on the ungated `/profile` page —
   and stripping a user's role narrows every key they hold immediately.
   `scopes: NULL` means "whatever the owner has", not "everything".

2. **`forms/edit.rs` is positional only, and that is the whole safety
   argument.** `add_element`/`update_element`/`remove_element`/`move_element`
   are pure `Vec<FormElement> -> CoreResult<Vec<FormElement>>`. The result goes
   straight back through `service::update_form`, so a helper cannot produce a
   layout the REST API would reject and cannot let the legacy columns go stale.
   The only errors they raise themselves are addressing ones ("no field with
   that key", "index 9 in a layout of 4") — the things `validate_layout` cannot
   phrase usefully. `crates/mcp/tests/tools.rs` pins both halves.

   Rows move and delete **as a unit**: naming either marker acts on the whole
   `row_start..=row_end` run. Rather than repair a split pair afterwards the way
   the builder's `normalizeRows` does, the operation that would create one
   doesn't exist.

3. **Tool schemas are spliced from the `utoipa` derives, not derived a second
   time.** rmcp wants `schemars::JsonSchema`; the form DTOs have
   `utoipa::ToSchema`, which already generates `/openapi.json` and the TS
   client. `forms/schema.rs` hands over the transitively-closed component set
   and `crates/mcp/src/schema.rs` rewrites `#/components/schemas/X` →
   `#/$defs/X`, which `#[tool(input_schema = …)]` accepts verbatim. Two
   artefacts, one generator — the `gen-regions.mjs` stance, not the
   `visibility.rs`/`visibility.ts` one. The `$defs` block is attached only to
   tools that actually `$ref` into it. Tests assert no published tool schema
   contains a dangling `$ref`.

4. **`#[tool_handler(router = self.tool_router)]` is load-bearing.** Left to
   its default the macro expands to `Self::tool_router()` — rebuilding all
   twelve tools, schemas included, on *every* `tools/list` and `tools/call`.
   Same for `Implementation::new(...)` over `from_build_env()`, which reads
   `CARGO_PKG_*` at its own expansion site inside rmcp and would report the SDK
   as the server.

5. **Over HTTP the actor arrives inside `http::request::Parts`, one level
   down.** `routes/mcp.rs` authenticates in middleware (401 before rmcp sees
   anything) and inserts an `ApiActor` into the request extensions; rmcp
   forwards the inbound request into `RequestContext::extensions` as a whole
   `Parts`, **not** as individual types, so `ctx.extensions.get::<ApiActor>()`
   would compile and always return `None`. `OpenRelayMcp::actor` is the other
   half of that contract — the two change together. rmcp does this on **POST**
   only, which covers `initialize` and every `tools/call`. The rmcp examples
   stop at "401 in middleware" and never propagate a principal, so there is no
   upstream pattern to follow here.

6. **rmcp's `Host` check defaults to loopback-only and fails closed.** A
   deployed server would reject every request until `allowed_hosts` is set;
   `routes/mcp.rs` derives it from `public_api_url`/`admin_url` (including the
   bare form of an explicit `:443`/`:80`, which a `Host` header omits) rather
   than adding another env var.

The stdio binary (`apps/mcp`) talks to MySQL directly and resolves one key at
startup — right for a local or same-host agent, wrong for anything remote,
which should use the HTTP endpoint. It logs to **stderr**; stdout is the
protocol channel.

```bash
# stdio
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  OPEN_RELAY_API_KEY=orl_... cargo run -p open-relay-mcp-stdio

# HTTP (no proxy binary needed — Claude Code and Claude Desktop both speak it)
claude mcp add --transport http open-relay http://localhost:8080/api/v1/mcp \
  --header "Authorization: Bearer orl_..."

# a dev key without going through the UI
DATABASE_URL=... cargo run -p open-relay-core --example mint_key -- 1 agent forms:read

DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-core --test api_keys -- --ignored
DATABASE_URL=mysql://root:openrelay@127.0.0.1:3306/openrelay \
  cargo test -p open-relay-mcp --test tools -- --ignored
```

## Conventions

- Editor config: 2-space indent everywhere except Rust (4) and Makefiles (tabs); LF line endings; final newline required.
- Cargo deps live in workspace `[workspace.dependencies]` — crates reference them with `{ workspace = true }`.
- Core services return `CoreError` (`crates/core/src/error.rs`) — framework-agnostic, no HTTP. Server errors funnel through `AppError` (`apps/server/src/error.rs`) which `From<CoreError>` lifts into HTTP responses via `IntoResponse`. `AppResult<T>` is the standard handler return type; new HTTP-only variants get an `IntoResponse` mapping in `AppError`.
- Wire-contract DTOs (`NewUser`, `UserDto`, `LoginRequest`/`LoginResponse`, `InitializeResponse`, `SetupStatus`, …) live in `crates/core` alongside the services that produce/consume them. `serde` and `utoipa::ToSchema` are pure metadata — they don't pull a framework in. Handlers just `Json<core::…::Foo>` them.
- Anything Axum-coupled (extractors, `OpenApiRouter` wiring, `IntoResponse`, the `utoipa-axum` / `utoipa-swagger-ui` glue) belongs in `apps/server`. Anything reusable/domain-shaped — persistence, validation, JWT issuance, the `Backend` and `Provider` traits, request/response shapes — belongs in `crates/core`. A non-HTTP caller (CLI seed command, worker) should be able to call core directly without touching the server crate.
