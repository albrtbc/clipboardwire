# syntax=docker/dockerfile:1
#
# Headless clipboardwire hub image.
#
# Stage 1 builds the GUI-free `clipboardwire-server` binary (depends only on
# clipboardwire-core; no tray/GTK/X11). Stage 2 ships it on distroless/cc,
# which provides glibc + libgcc and nothing else — no shell, no package
# manager, minimal attack surface. Final image is ~30 MB.
#
# Build:  docker build -t clipboardwire-server .
# Run:    see docker-compose.yml

FROM rust:1.89-bookworm AS build
# aws-lc-rs (rustls' default crypto provider, pulled in via core) needs a C
# toolchain + cmake to build its vendored sources.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
# Cache the cargo registry and target dir across builds (BuildKit). The binary
# is copied out within the same layer because the cache mount is not persisted
# into the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p clipboardwire-server \
    && cp target/release/clipboardwire-server /clipboardwire-server

FROM gcr.io/distroless/cc-debian12
COPY --from=build /clipboardwire-server /clipboardwire-server
# 8484 is the default CLIPBOARDWIRE_BIND port.
EXPOSE 8484
# distroless has no shell, so this is the exec form (required).
ENTRYPOINT ["/clipboardwire-server"]
