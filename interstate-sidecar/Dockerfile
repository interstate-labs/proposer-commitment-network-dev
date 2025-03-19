FROM rust:1.82.0-slim-bullseye AS compiler 

RUN cargo install cargo-chef

WORKDIR /app

FROM compiler AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM compiler AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN apt-get update && apt-get install pkg-config libssl-dev -y
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM debian@sha256:6344a6747740d465bff88e833e43ef881a8c4dd51950dba5b30664c93f74cbef
WORKDIR /usr/local/bin
COPY --from=builder /app/target/release/interstate-pbs-module /
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Create a non-root user and switch to it
RUN useradd -m appuser
USER appuser

# Mount allowlisted directories as read-only volumes
VOLUME ["/keystores:ro", "/work/interstate-protocol/proposer-commitment-network/.env:ro"]

ENTRYPOINT ["/interstate-pbs-module"]
CMD ["--read-only"]
