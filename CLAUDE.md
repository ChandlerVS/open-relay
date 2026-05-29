# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Skeleton. Boot wiring is in place (server, schema sync, OpenAPI, embed SDK, admin SPA) but no domain resources exist yet. Planned order: Users → Forms → Backends → Submissions. Most route handlers and the delivery worker are intentional no-op stubs that return `NotImplemented` or log a tick.

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

# Backend (binds 0.0.0.0:8080 by default; serves /healthz, /openapi.json, /docs)
cp .env.example .env   # first time only
cargo run -p open-relay-server

# Frontend
pnpm install
pnpm gen:api           # snapshots openapi.json → packages/api-client (server MUST be running)
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

### Backend delivery is a registry of trait objects

`open_relay_core::backend::Backend` is the integration surface (GoHighLevel, OpenRelay's own store, etc.). Implementations register against the `BackendRegistry` held in `AppState`. The registry is currently constructed empty in `AppState::new` (`apps/server/src/state.rs`) — concrete backends should be registered there at boot.

`DeliveryError` distinguishes `Transient` (worker retries) from `Permanent` (no retry, admin notify). `Backend::deliver` must be idempotent on `submission_id`.

### Delivery worker is a no-op stub

`crates/core/src/jobs/worker.rs` spawns a tokio loop that will eventually poll `submission_delivery` rows with `SELECT … FOR UPDATE SKIP LOCKED`. Today it just logs a tick every 30s. The `submission_delivery` entity does not exist yet; wiring it up unblocks the worker.

### Auth is local JWT + pluggable Provider trait

- `apps/server/src/auth/local.rs` — `POST /auth/login`, currently returns `NotImplemented` because the `user` entity doesn't exist.
- `apps/server/src/auth/provider.rs` — `Provider` trait + `ProviderRegistry` for OAuth/SSO. The `oauth2` crate is in deps; no concrete providers ship in the skeleton.
- `AuthUser` Axum extractor verifies a `Bearer <jwt>` header against `AppState::auth_keys`.

### Embed SDK isolates via Shadow DOM

`apps/embed-sdk` builds to a single IIFE (`open-relay.js`) with React/ReactDOM bundled in (no peer-dep on the host page). At runtime it reads `data-form-id`/`data-api-url` off the executing `<script>`, inserts a sibling `<div>`, attaches an open shadow root, and applies its CSS via a constructable stylesheet so the host page's styles can't bleed in.

### Tailwind v4

The admin uses Tailwind v4 via `@tailwindcss/vite` (no `tailwind.config.js` — config is CSS-driven). The embed SDK uses plain CSS imported with `?inline` for shadow-root injection.

### TypeScript

All TS packages extend `tsconfig.base.json` (strict, `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, `noEmit`). Build is via Vite or tsgo (TS 6); `pnpm typecheck` runs `tsc --noEmit` everywhere.

## Conventions

- Editor config: 2-space indent everywhere except Rust (4) and Makefiles (tabs); LF line endings; final newline required.
- Cargo deps live in workspace `[workspace.dependencies]` — crates reference them with `{ workspace = true }`.
- Server errors funnel through `AppError` in `apps/server/src/error.rs`. New error variants get an `IntoResponse` mapping there. `AppResult<T>` is the standard handler return type.
- Anything HTTP-shaped belongs in `apps/server`. Anything reusable/domain-shaped belongs in `crates/core`.
