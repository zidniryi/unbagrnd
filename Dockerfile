# syntax=docker/dockerfile:1

# Multi-stage build for unbagrnd-api, the self-hosted REST API.
#
# Only `-p unbagrnd-api` is ever built here - Cargo only compiles that
# package and its dependency graph (unbagrnd-core), never the Tauri desktop
# app in src-tauri/, so none of its GTK/webkit system dependencies are
# needed in this image at all.
#
# `ort`'s `download-binaries` feature statically links the ONNX Runtime
# *code* into the binary at build time (see core/Cargo.toml), but it still
# dynamically links against the system's libstdc++ - and the prebuilt ONNX
# Runtime library `ort` downloads was built with a newer GCC than Debian
# bookworm ships (missing libstdc++ symbols like `_M_replace_cold` show up
# as "undefined reference" link errors on bookworm). Both stages use
# trixie (Debian 13) instead, whose libstdc++ is new enough.

########## Builder ##########
FROM rust:1-trixie AS builder
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# The whole workspace is needed for Cargo to resolve it (root Cargo.toml
# lists src-tauri/core/api as members), even though src-tauri itself is
# never compiled - see .dockerignore for what's excluded (target/,
# node_modules/, src-tauri/gen/android build outputs, ...).
COPY . .

RUN cargo build --release -p unbagrnd-api

########## Runtime ##########
FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /home/unbagrnd unbagrnd

COPY --from=builder /build/target/release/unbagrnd-api /usr/local/bin/unbagrnd-api

RUN mkdir -p /data /models && chown -R unbagrnd:unbagrnd /data /models
USER unbagrnd
WORKDIR /home/unbagrnd

ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATABASE_URL=sqlite:///data/unbagrnd.db \
    UNBAGRND_MODELS_DIR=/models

EXPOSE 8080
VOLUME ["/data", "/models"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1

ENTRYPOINT ["unbagrnd-api"]
