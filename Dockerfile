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

RUN cargo chef cook --release --locked --recipe-path recipe.json --package comettrace \
    ${FEATURES:+--features "$FEATURES"}

COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY apps apps

RUN cargo build --release --locked --package comettrace \
    ${FEATURES:+--features "$FEATURES"}

FROM node:22-alpine@sha256:fe4f6d74e20370acb04ad889077d14d0f64a9c8ea6a42bc60cddf1e844699914 AS web-build

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
