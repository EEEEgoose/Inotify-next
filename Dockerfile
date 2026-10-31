# syntax=docker/dockerfile:1

# 1. Web UI
FROM node:22-alpine AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# 2. Server, with the web UI embedded
FROM rust:1.96-bookworm AS server
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
