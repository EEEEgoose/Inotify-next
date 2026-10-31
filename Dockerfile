# syntax=docker/dockerfile:1

# 1. Web UI
# Base images are pinned by digest; Dependabot (.github/dependabot.yml) proposes updates.
FROM node:22-alpine@sha256:fe4f6d74e20370acb04ad889077d14d0f64a9c8ea6a42bc60cddf1e844699914 AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# 2. Server, with the web UI embedded
FROM rust:1.96-bookworm@sha256:a339861ae23e9abb272cea45dfafde21760d2ce6577a70f8a926153677902663 AS server
WORKDIR /src/server
COPY server/ ./
COPY --from=web /src/web/dist /src/web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/server/target \
    cargo build --release --locked \
    && cp target/release/inotify /inotify
# Owned by the runtime user, so a fresh named volume mounted here is writable.
RUN mkdir /data

# 3. Runtime: no shell, runs as non-root (uid 65532)
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=server /inotify /usr/local/bin/inotify
COPY --from=server --chown=65532:65532 /data /data
ENV INOTIFY_DATA_DIR=/data \
    INOTIFY_LISTEN=0.0.0.0:8000
VOLUME /data
EXPOSE 8000
ENTRYPOINT ["/usr/local/bin/inotify"]
