FROM rust:1.85-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY crates/yellowstone-mock/Cargo.toml crates/yellowstone-mock/
COPY crates/clob-engine/Cargo.toml      crates/clob-engine/
COPY crates/clob-api/Cargo.toml         crates/clob-api/

# Stub all lib/main entries so workspace resolves without full source.
RUN mkdir -p crates/clob-engine/src      && echo "" > crates/clob-engine/src/lib.rs \
 && mkdir -p crates/clob-api/src         && echo "fn main(){}" > crates/clob-api/src/main.rs \
 && mkdir -p crates/yellowstone-mock/src && echo "fn main(){}" > crates/yellowstone-mock/src/main.rs

RUN cargo build --release -p yellowstone-mock 2>/dev/null || true

COPY crates/yellowstone-mock/src crates/yellowstone-mock/src

RUN cargo build --release -p yellowstone-mock

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/yellowstone-mock /usr/local/bin/yellowstone-mock
ENTRYPOINT ["yellowstone-mock"]