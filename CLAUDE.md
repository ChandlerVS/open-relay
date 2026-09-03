# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Functional end-to-end for the core flow. Boot wiring (server, schema sync, OpenAPI, embed SDK, admin SPA) plus the domain resources — Users, Forms, Backends, Submissions — are implemented, along with auth/RBAC, OAuth provider config, secrets-at-rest, and the delivery worker. Route handlers call into `crates/core` services; `NotImplemented` is just an `AppError` variant, not a stubbed handler. Still evolving: concrete delivery backends beyond the built-ins, more OAuth/SSO providers, and broader admin UX.

`OpenRelay.md` is the engineering design doc — it is gitignored, so consult it for intent but don't expect collaborators to have it.

## Stack & layout

Hybrid Cargo + pnpm/Turborepo monorepo.

- `apps/server/` — Axum HTTP API + delivery worker (Rust, edition 2024). Bin: `open-relay-server`.
- `crates/entity/` — SeaORM 2.0 entities. Hand-authored.
- `crates/core/` — Framework-agnostic domain logic (`Backend` trait, registry, delivery worker). Must not depend on Axum.
- `apps/admin/` — Vite + React 19 admin SPA (port 5173).
- `apps/embed-sdk/` — Vite library-mode IIFE bundle, dropped into host pages via `<script>`.
- `packages/api-client/` — OpenAPI-generated TS client (consumed by admin).
- `packages/form-renderer/` — Shared React form components (admin preview + embed SDK).
- `packages/ui/` — shadcn-style primitives (admin only).
- `infra/docker-compose.yml` — Local MySQL 8.

## Commands

Prereqs: Rust (edition 2024), Node 22.11 (`nvm use`), pnpm 10, Docker.

```bash
# Local MySQL (required before server start)
docker compose -f infra/docker-compose.yml up -d mysql

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

### Backend delivery is a registry of trait objects

`open_relay_core::backend::Backend` is the integration surface (GoHighLevel, OpenRelay's own store, etc.). Implementations register against the `BackendRegistry` held in `AppState`, constructed in `AppState::new` (`apps/server/src/state.rs`) — it registers `OpenRelayBackend` (static) and `GoHighLevelFactory` at boot today. New backends register there: `register_static` for config-less backends, `register_factory` for ones built per `backend_instance` row.

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

## Conventions

- Editor config: 2-space indent everywhere except Rust (4) and Makefiles (tabs); LF line endings; final newline required.
- Cargo deps live in workspace `[workspace.dependencies]` — crates reference them with `{ workspace = true }`.
- Core services return `CoreError` (`crates/core/src/error.rs`) — framework-agnostic, no HTTP. Server errors funnel through `AppError` (`apps/server/src/error.rs`) which `From<CoreError>` lifts into HTTP responses via `IntoResponse`. `AppResult<T>` is the standard handler return type; new HTTP-only variants get an `IntoResponse` mapping in `AppError`.
- Wire-contract DTOs (`NewUser`, `UserDto`, `LoginRequest`/`LoginResponse`, `InitializeResponse`, `SetupStatus`, …) live in `crates/core` alongside the services that produce/consume them. `serde` and `utoipa::ToSchema` are pure metadata — they don't pull a framework in. Handlers just `Json<core::…::Foo>` them.
- Anything Axum-coupled (extractors, `OpenApiRouter` wiring, `IntoResponse`, the `utoipa-axum` / `utoipa-swagger-ui` glue) belongs in `apps/server`. Anything reusable/domain-shaped — persistence, validation, JWT issuance, the `Backend` and `Provider` traits, request/response shapes — belongs in `crates/core`. A non-HTTP caller (CLI seed command, worker) should be able to call core directly without touching the server crate.
