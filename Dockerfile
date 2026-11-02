# syntax=docker/dockerfile:1

FROM rust:1.97.0-trixie@sha256:b92b8c8574f8f3b207fcb0912fb3e2de4041580b5934d90312d53938c9a038a9 AS chef

WORKDIR /usr/local/src/comettrace

RUN cargo install cargo-chef --version 0.1.77 --locked

FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY apps apps

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ARG FEATURES=""

COPY --from=planner /usr/local/src/comettrace/recipe.json recipe.json

RUN cargo chef cook --release --locked --recipe-path recipe.json --package ferristrace-server \
    ${FEATURES:+--features "$FEATURES"}

COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY apps apps

RUN cargo build --release --locked --package ferristrace-server \
    ${FEATURES:+--features "$FEATURES"}

FROM node:26-alpine@sha256:c6ad9e9c1d80f6eb607c26b4b0a132ff7a2344fad1c78ea3baa2a7d4c8de6d94 AS web-build

WORKDIR /usr/local/src/comettrace/web

RUN corepack enable

COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./

RUN pnpm install --frozen-lockfile

COPY web/ ./

RUN pnpm build

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1 AS runtime

COPY --from=builder /usr/local/src/comettrace/target/release/comettrace /usr/local/bin/comettrace
COPY --from=web-build /usr/local/src/comettrace/web/dist /usr/local/share/comettrace/web

ENV RUST_LOG=info
ENV COMETTRACE_WEB_ROOT=/usr/local/share/comettrace/web

EXPOSE 4317 4318

ENTRYPOINT ["comettrace"]
