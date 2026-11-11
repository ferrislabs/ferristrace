# syntax=docker/dockerfile:1

# One file, one image per target: server (the default), mcp, mcp-gateway.
# The three binaries come out of one Rust build, so building every target
# compiles the workspace once.

FROM rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 AS chef

WORKDIR /usr/local/src/ferristrace

RUN cargo install cargo-chef --version 0.1.77 --locked

FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY apps apps

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /usr/local/src/ferristrace/recipe.json recipe.json

RUN cargo chef cook --release --locked --recipe-path recipe.json \
    --package ferristrace-server --package ferristrace-mcp --package ferristrace-mcp-gateway

COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY apps apps

RUN cargo build --release --locked \
    --bin ferristrace --bin ferristrace-mcp --bin ferristrace-mcp-gateway

FROM node:26-alpine@sha256:8996bb5821d62a8fd9bb27c1cb07462f3fdce6e47d640c6a881ead0b064ce216 AS web-build

WORKDIR /usr/local/src/ferristrace/web

RUN npm install --global corepack@0.36.0 && corepack enable

COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./

RUN pnpm install --frozen-lockfile

COPY web/ ./

RUN pnpm build

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:cc66d222f0a7ec996b78a99fe76a969bc80b539a5016ad866c14d1949bc6d0ab AS runtime

ENV RUST_LOG=info

FROM runtime AS mcp

COPY --from=builder /usr/local/src/ferristrace/target/release/ferristrace-mcp /usr/local/bin/ferristrace-mcp

ENV FERRISTRACE_MCP_LISTEN=0.0.0.0:8081

EXPOSE 8081

ENTRYPOINT ["ferristrace-mcp"]

FROM runtime AS mcp-gateway

COPY --from=builder /usr/local/src/ferristrace/target/release/ferristrace-mcp-gateway /usr/local/bin/ferristrace-mcp-gateway

ENV FERRISTRACE_MCP_GATEWAY_LISTEN=0.0.0.0:8080

EXPOSE 8080

ENTRYPOINT ["ferristrace-mcp-gateway"]

FROM runtime AS server

COPY --from=builder /usr/local/src/ferristrace/target/release/ferristrace /usr/local/bin/ferristrace
COPY --from=web-build /usr/local/src/ferristrace/web/dist /usr/local/share/ferristrace/web

ENV FERRISTRACE_WEB_ROOT=/usr/local/share/ferristrace/web

EXPOSE 4317 4318

ENTRYPOINT ["ferristrace"]
