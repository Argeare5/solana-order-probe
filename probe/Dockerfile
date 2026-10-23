# order-probe: the Rust binary that snapshots the cluster, sends probe bursts and collects their blocks.
# Keys and raw data are volumes (see docker-compose.yml); the image holds no secrets.

FROM rust:1.99.1-slim-bookworm AS build
# aws-lc-sys (rustls' crypto backend) builds C code
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang perl make pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/order-probe /usr/local/bin/order-probe

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/order-probe /usr/local/bin/order-probe
WORKDIR /work
ENV DATA_DIR=/work/data SECRETS_DIR=/work/secrets
ENTRYPOINT ["order-probe"]
CMD ["--help"]
