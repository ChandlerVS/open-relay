# OpenRelay

Embeddable form orchestration. A Rust backend routes submissions to configurable backends (OpenRelay, GoHighLevel, …); a React admin panel manages forms and submissions; a React-based embed SDK renders forms inside any host website via Shadow DOM.

## Repo layout

```
apps/
  server/       # Axum HTTP API + queue worker
  admin/        # Vite + React admin SPA
  embed-sdk/    # Vite library-mode embed script (IIFE)
crates/
  entity/       # SeaORM 2.0 entities (hand-authored)
  core/         # Domain logic: Backend trait, queue worker
packages/
  api-client/   # OpenAPI-generated TS client
  form-renderer/ # Shared React form components (admin preview + embed SDK)
  ui/           # shadcn/ui primitives (admin)
infra/
  docker-compose.yml   # local MySQL 8
```

## Quickstart

Prereqs: Rust (edition 2024), Node 22+ via `nvm use`, pnpm 10, Docker.

```bash
# 1. Start MySQL
docker compose -f infra/docker-compose.yml up -d mysql

# 2. Backend
cp .env.example .env
cargo run -p open-relay-server
# -> listens on http://localhost:8080
#    /healthz, /openapi.json, /docs (Swagger UI)

# 3. Frontend (in a second terminal)
pnpm install
pnpm gen:api      # snapshots openapi.json -> packages/api-client
pnpm dev          # admin (http://localhost:5173) + embed-sdk watch build
```

## Status

Skeleton only — boot wiring in place, no domain resources yet. Next: Users → Forms → Backends → Submissions.
