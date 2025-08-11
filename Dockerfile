# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.88.0

FROM rust:${RUST_VERSION}-alpine AS build
WORKDIR /app

RUN apk add --no-cache clang lld musl-dev git

RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
cargo build --locked --release && \
cp ./target/release/incydecy /bin/incydecy

FROM alpine:3.18 AS final

COPY --from=build /bin/incydecy /bin/incydecy

WORKDIR /app

CMD ["/bin/incydecy"]
