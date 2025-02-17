FROM rust:1.82.0-slim-bullseye AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# Build the project
RUN cargo build --release

# Runtime image
FROM debian:bullseye-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy built binary and config
COPY --from=builder /app/target/release/interstate-boost /usr/local/bin/
COPY --from=builder /app/is-config.toml /app/

# Create non-root user
RUN useradd -m appuser && \
    chown -R appuser:appuser /app

USER appuser

ENTRYPOINT ["interstate-boost"]