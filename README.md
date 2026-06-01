# OpenRelay

**Version 0.1.0**

**Embeddable form orchestration.** Drop a `<script>` tag onto any website to render a form, then route each submission to one or more configurable backends (OpenRelay's own store, GoHighLevel, …) with durable, retrying delivery. A React admin panel manages forms, backends, users, and submissions.

- **Rust API + delivery worker** (Axum) — collects submissions and delivers them asynchronously with at-least-once semantics.
- **React admin SPA** — form builder, backend configuration, submission review, user/role management.
- **Embeddable SDK** — a single `open-relay.js` IIFE that renders forms inside any host page, isolated via Shadow DOM so the host's CSS can't bleed in.

## Features

- **Embed anywhere** — paste one `<script>` tag; the SDK fetches the form schema and renders it in an isolated shadow root. No peer dependencies on the host page.
- **Pluggable delivery backends** — submissions fan out to a registry of `Backend` trait objects. Delivery is asynchronous, idempotent per submission, and retried with exponential backoff.
- **Durable queue** — the worker leases due rows with `SELECT … FOR UPDATE SKIP LOCKED`, retries transient failures (30s → 24h backoff over 6 attempts), and reclaims stale leases on restart.
- **Local auth + pluggable SSO** — local JWT login with Argon2 password hashing and refresh tokens, plus an OAuth `Provider` trait for admin-configured SSO.
- **RBAC** — roles, a permission catalogue, and per-route permission checks.
- **Secrets encrypted at rest** — backend tokens and OAuth client secrets are sealed with ChaCha20-Poly1305 (AEAD) before they touch the database.
- **Self-documenting API** — OpenAPI spec generated from route attributes; Swagger UI at `/docs` in development.
- **Single-binary deploy** — one all-in-one Docker image serves the API, the embed SDK, and the admin SPA on one origin. MySQL is the only external dependency.

## Architecture

```
                    ┌─────────────────┐
  host website ───▶ │  embed-sdk      │  open-relay.js (Shadow DOM)
  (any origin)      └────────┬────────┘
                             │ GET schema / POST submission (CORS: any origin)
                             ▼
┌──────────────────────────────────────────────────────────┐
│  apps/server  (Axum)                                       │
│    public API ──▶ submissions ──▶ submission_delivery queue│
│    admin API  ──▶ forms / backends / users / auth          │
│    delivery worker ──poll/lease──▶ Backend registry ──▶ ┐  │
└─────────────────────────────────────────────────────────┼──┘
        │ SeaORM (schema synced at boot)                    │ deliver()
        ▼                                                   ▼
     MySQL 8                              GoHighLevel · OpenRelay store · …
```

The HTTP layer (Axum extractors, OpenAPI wiring, `IntoResponse`) lives in `apps/server`. Everything reusable and framework-agnostic — persistence, validation, JWT issuance, the `Backend`/`Provider` traits, the delivery worker, request/response DTOs — lives in `crates/core`, so a non-HTTP caller (CLI, worker) can use it directly.

## Repo layout

Hybrid Cargo + pnpm/Turborepo monorepo.

```
apps/
  server/         # Axum HTTP API + delivery worker (Rust, edition 2024). bin: open-relay-server
  admin/          # Vite + React 19 admin SPA (port 5173)
  embed-sdk/      # Vite library-mode embed script (IIFE, React bundled in)
crates/
  entity/         # SeaORM 2.0 entities (hand-authored, schema synced at boot)
  core/           # Domain logic: auth, RBAC, forms, submissions, Backend/Provider traits, delivery worker, crypto
packages/
  api-client/     # OpenAPI-generated TypeScript client (consumed by admin)
  form-renderer/  # Shared React form components (admin preview + embed SDK)
  ui/             # shadcn/ui-style primitives (admin only)
infra/
  docker-compose.yml   # local MySQL 8
```

## Quickstart

Prereqs: Rust (edition 2024, rustc ≥ 1.85), Node 22.11 (`nvm use`), pnpm 10, Docker.

```bash
# 1. Start MySQL
docker compose -f infra/docker-compose.yml up -d mysql

# 2. Configure + run the backend
cp .env.example .env
# Generate the two required secrets (must differ from each other):
#   openssl rand -base64 32   # -> JWT_SECRET
#   openssl rand -base64 32   # -> ENCRYPTION_KEY
cargo run -p open-relay-server
# -> http://localhost:8080  (/healthz, /openapi.json, /docs)

# 3. Frontend (second terminal)
pnpm install
pnpm gen:api      # snapshots openapi.json -> packages/api-client (server must be running)
pnpm dev          # admin on http://localhost:5173 + embed-sdk watch build
```

The schema is synced into MySQL on boot (idempotent, additive) — there's no migration step. On first run, bootstrap the initial admin user via `POST /setup/initialize` (one-shot; returns 409 once a user exists), then sign in through the admin SPA.

## Configuration

All config is environment-driven (see [`.env.example`](.env.example) for the annotated list). Key variables:

| Variable | Required | Description |
| --- | --- | --- |
| `DATABASE_URL` | ✅ | MySQL connection string. |
| `JWT_SECRET` | ✅ | ≥ 32 bytes of high-entropy random data for signing local-auth JWTs. The server refuses to boot with a short/placeholder value. |
| `ENCRYPTION_KEY` | ✅ | Base64 that decodes to exactly 32 bytes; AEAD key for secrets at rest. **Must differ from `JWT_SECRET`.** |
| `ENVIRONMENT` | | `development` or `production` (default). Development enables Swagger UI / `/openapi.json` and relaxes the SSRF guard for localhost OAuth. |
| `LISTEN_ADDR` | | Server bind address (default `0.0.0.0:8080`). |
| `PUBLIC_API_URL` | | Base URL the API is reachable at from browsers (OAuth `redirect_uri`). |
| `ADMIN_URL` | | Admin SPA origin; the credentialed CORS allow-list and post-OAuth redirect target. |
| `EMBED_SDK_URL` | | URL the embed `<script>` snippet points at (defaults to `{PUBLIC_API_URL}/embed/open-relay.js`). |
| `EMBED_SDK_PATH` / `ADMIN_DIST_PATH` | | Filesystem paths the server serves the embed bundle / admin SPA from. |
| `COOKIE_SECURE` | | Set the `Secure` attribute on the OAuth state cookie (false for plain-HTTP local dev). |
| `RUST_LOG` | | `tracing-subscriber` `EnvFilter` directive. |

## Docker (all-in-one)

A single image runs the whole stack (API + embed SDK + admin SPA) on one origin; MySQL is the only external dependency.

```bash
docker build -t open-relay .
docker run -p 8080:8080 --env-file .env open-relay
```

`DATABASE_URL`, `JWT_SECRET`, `ENCRYPTION_KEY`, `PUBLIC_API_URL`, and `ADMIN_URL` have no safe defaults and must be supplied at runtime. With the SPA served same-origin, set `ADMIN_URL == PUBLIC_API_URL`. HSTS is emitted in production and assumes a TLS-terminating proxy in front.

## Common commands

```bash
cargo run -p open-relay-server        # run the API + worker
cargo test -p open-relay-core         # core unit tests
pnpm build                            # turbo build (respects ^build ordering)
pnpm typecheck                        # tsc --noEmit across all TS packages
pnpm gen:api                          # regenerate the TS API client from a running server
```

After adding or changing a route, restart the server and re-run `pnpm gen:api` so the committed TypeScript client stays in sync with the OpenAPI spec.

## How it fits together (notes for contributors)

- **Entity-first schema.** Entities are hand-authored Rust types under `crates/entity/src/`; the schema is derived from them and synced at boot via `db.get_schema_registry("entity::*").sync(&db)`. Do **not** use `sea-orm-cli generate`. Adding an entity is two steps: create the file, add `pub mod <name>;` to `lib.rs` — the `entity::*` glob auto-discovers it.
- **OpenAPI from attributes.** A handler appears in `/openapi.json` only if it carries `#[utoipa::path(...)]` and is passed to the `routes!` macro. Tags on `ApiDoc` (`apps/server/src/router.rs`) must match the `tag = "..."` strings on handlers.
- **Backends are a registry of trait objects.** Implement `open_relay_core::backend::Backend` and register it against the `BackendRegistry` in `AppState`. `Backend::deliver` must be idempotent on `submission_id`; `DeliveryError::Transient` is retried, `Permanent` is not.
- **Layering rule.** Anything Axum-coupled lives in `apps/server`; anything reusable/domain-shaped lives in `crates/core`, which must not depend on Axum.

See [`CLAUDE.md`](CLAUDE.md) for the full set of architecture conventions.

## Status

Functional and end-to-end for the core flow: embed → collect → durable, retrying delivery, with auth, RBAC, forms/backends/submissions management, and the admin SPA all in place.

Still evolving: concrete delivery backends beyond the built-ins, additional OAuth/SSO providers, and broader admin UX. APIs may change before a tagged release — pin a commit if you build on it.

## Scaling limitations

This is an early (0.1.0) release tuned for correctness and simplicity over throughput. Known limitations:

- **No cache layer.** Every form submission writes synchronously to MySQL on the request path, and form schemas are read from the database on each fetch rather than served from an in-memory or distributed cache. Under high submission volume the database becomes the bottleneck.
- **Database-backed queue.** The delivery queue is the `submission_delivery` table polled with `SELECT … FOR UPDATE SKIP LOCKED`. This is durable and simple but couples queue throughput to database capacity; it is not a substitute for a dedicated broker at scale.
- **Single-instance worker assumptions.** The delivery worker leases rows safely across instances, but there is no horizontal autoscaling story, backpressure signalling, or rate limiting on the public submission endpoint yet.

These are on the roadmap. Planned work includes a cache layer in front of form-schema reads and submission writes (e.g. a write-buffer / ingestion queue so submissions are acknowledged without a blocking DB write), and the option to back the delivery queue with a dedicated message broker.

## Contributing

Issues and pull requests are welcome. Please run `cargo test`, `pnpm typecheck`, and `cargo fmt` before opening a PR, and keep framework-agnostic logic in `crates/core`.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option. Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion shall be dual-licensed as above, without any additional terms or conditions.
