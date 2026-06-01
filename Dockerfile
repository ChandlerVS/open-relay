# syntax=docker/dockerfile:1

# All-in-one OpenRelay image: a single process (the Rust `open-relay-server`
# binary) serves the JSON API and the embed SDK bundle (both under `/api/v1`,
# e.g. `/api/v1/embed/open-relay.js`) plus the admin SPA (catch-all fallback) on
# one origin/port. The only external dependency is MySQL.
#
# Build:   docker build -t open-relay .
# Run:     docker run -p 8080:8080 --env-file .env open-relay
#          (DATABASE_URL must point at a reachable MySQL; the schema is synced
#           on boot, so no migration step is needed.)

# ---------------------------------------------------------------------------
# Stage 1 — build the JS bundles (admin SPA + embed SDK) with pnpm/Turborepo.
# The OpenAPI TS client is committed (packages/api-client/src/generated), so
# this builds fully offline — no running server required.
# ---------------------------------------------------------------------------
FROM node:22-bookworm-slim AS web
ENV PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH
RUN corepack enable
WORKDIR /repo

# Manifests first so `pnpm install` is cached across source-only changes.
COPY pnpm-workspace.yaml package.json pnpm-lock.yaml turbo.json tsconfig.base.json ./
COPY apps/admin/package.json ./apps/admin/
COPY apps/embed-sdk/package.json ./apps/embed-sdk/
COPY packages/api-client/package.json ./packages/api-client/
COPY packages/form-renderer/package.json ./packages/form-renderer/
COPY packages/ui/package.json ./packages/ui/
RUN --mount=type=cache,target=/pnpm/store pnpm install --frozen-lockfile

# Source + build. Empty VITE_API_URL makes the admin call the API with
# same-origin relative URLs, which is exactly how it's served in this image.
COPY . .
ENV VITE_API_URL=""
RUN pnpm build

# ---------------------------------------------------------------------------
# Stage 2 — compile the Rust server (edition 2024 → needs Rust >= 1.85).
# ---------------------------------------------------------------------------
FROM rust:1-slim-bookworm AS server
    # curl is needed at build time: utoipa-swagger-ui's build script downloads
    # the Swagger UI archive with it (the /docs UI, mounted dev-only at runtime).
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config build-essential ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src

# Cargo workspace: root manifest/lock + the three member crates.
COPY Cargo.toml Cargo.lock ./
COPY apps/server ./apps/server
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p open-relay-server \
    && cp target/release/open-relay-server /open-relay-server

# ---------------------------------------------------------------------------
# Stage 3 — slim runtime: just the binary + the built static assets.
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 -d /app appuser
WORKDIR /app

COPY --from=server /open-relay-server /usr/local/bin/open-relay-server
COPY --from=web /repo/apps/admin/dist ./admin
COPY --from=web /repo/apps/embed-sdk/dist ./embed
RUN chown -R appuser:appuser /app

# Baked-in defaults that point the server at the assets copied above. The
# secrets and URLs (DATABASE_URL, JWT_SECRET, ENCRYPTION_KEY, PUBLIC_API_URL,
# ADMIN_URL) have no safe defaults and MUST be supplied at run time — the
# server refuses to boot otherwise. With the SPA served same-origin, set
# ADMIN_URL == PUBLIC_API_URL.
ENV LISTEN_ADDR=0.0.0.0:8080 \
    ENVIRONMENT=production \
    ADMIN_DIST_PATH=/app/admin \
    EMBED_SDK_PATH=/app/embed/open-relay.js

EXPOSE 8080
USER appuser

HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/api/v1/healthz || exit 1

ENTRYPOINT ["open-relay-server"]
