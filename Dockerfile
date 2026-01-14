# Build stage
FROM rust:1.82-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock* ./

# Create dummy src for dependency caching
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies only
RUN cargo build --release && rm -rf src target/release/tpp*

# Copy actual source code
COPY src ./src

# Build the actual binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false tpp

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/tpp /app/tpp

# Set ownership
RUN chown -R tpp:tpp /app

USER tpp

# Default config path
ENV TPP_CONFIG=/app/config.yaml

# Expose ports
# 8080 - proxy
# 9090 - health check
EXPOSE 8080 9090

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9090/healthz || exit 1

ENTRYPOINT ["/app/tpp"]
CMD ["--config", "/app/config.yaml"]
