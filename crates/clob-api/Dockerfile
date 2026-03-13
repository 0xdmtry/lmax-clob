FROM rust:1.85-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY crates/clob-engine/Cargo.toml  crates/clob-engine/
COPY crates/clob-api/Cargo.toml     crates/clob-api/
COPY crates/yellowstone-mock/Cargo.toml crates/yellowstone-mock/

RUN mkdir -p crates/clob-engine/src      && echo "" > crates/clob-engine/src/lib.rs \
 && mkdir -p crates/clob-api/src         && echo "fn main(){}" > crates/clob-api/src/main.rs \
 && mkdir -p crates/yellowstone-mock/src && echo "fn main(){}" > crates/yellowstone-mock/src/main.rs

RUN cargo build --release -p clob-api 2>/dev/null || true

COPY crates/clob-engine/src  crates/clob-engine/src
COPY crates/clob-api/src     crates/clob-api/src
COPY migrations              migrations

RUN cargo build --release -p clob-api

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/clob-api /usr/local/bin/clob-api
ENTRYPOINT ["clob-api"]