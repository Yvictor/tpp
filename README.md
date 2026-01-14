# TPP - Token Pool Proxy

[![CI](https://github.com/Yvictor/tpp/actions/workflows/ci.yml/badge.svg)](https://github.com/Yvictor/tpp/actions/workflows/ci.yml)
[![Release](https://github.com/Yvictor/tpp/actions/workflows/release.yml/badge.svg)](https://github.com/Yvictor/tpp/actions/workflows/release.yml)
[![Docker](https://img.shields.io/docker/v/sinotrade/tpp?label=docker&sort=semver)](https://hub.docker.com/r/sinotrade/tpp)

A high-performance HTTP reverse proxy built with [Pingora](https://github.com/cloudflare/pingora) that provides Bearer token connection pooling for DolphinDB REST API.

## Features

- **Automatic Token Acquisition** - Uses a single credential to acquire N tokens (configurable `pool_size`) via `/api/login` at startup
- **Per-Connection Token Binding** - Each TCP connection is bound to a dedicated token for the duration of the connection
- **Connection Queuing** - When all tokens are in use, new connections wait indefinitely until a token becomes available
- **Auto Token Refresh** - Automatically refreshes tokens before they expire based on TTL
- **Health Check Endpoints** - Built-in `/health`, `/livez`, `/readyz`, and `/metrics` endpoints
- **OpenTelemetry Support** - Full observability with traces and metrics export

## Quick Start

### Using Docker

```bash
docker run -d \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -p 8080:8080 \
  -p 9090:9090 \
  sinotrade/tpp:latest
```

### From Source

```bash
# Clone the repository
git clone https://github.com/Yvictor/tpp.git
cd tpp

# Build
cargo build --release

# Run
./target/release/tpp --config config.yaml
```

## Configuration

Create a `config.yaml` file:

```yaml
# Proxy listen address
listen: "0.0.0.0:8080"

# Health check server (optional but recommended for k8s/docker)
health_listen: "0.0.0.0:9090"

# Upstream DolphinDB server
upstream:
  host: "dolphindb.example.com"
  port: 8848
  tls: false

# Single credential - will be used to acquire `pool_size` tokens
credential:
  username: "your_username"
  password: "your_password"

# Token pool configuration
token:
  pool_size: 200             # Number of tokens to acquire (default: 10)
  ttl_seconds: 3600          # Token TTL in seconds (default: 1 hour)
  refresh_check_seconds: 60  # How often to check for expired tokens

# Telemetry (optional)
telemetry:
  otlp_endpoint: "http://localhost:4317"
  log_filter: "info"
```

## How It Works

```
┌─────────┐     ┌─────────────┐     ┌────────────┐
│ Client  │────▶│  TPP:8080   │────▶│ DolphinDB  │
└─────────┘     └─────────────┘     └────────────┘
                      │
                      ▼
              ┌───────────────┐
              │  Token Pool   │
              │ ┌───┬───┬───┐ │
              │ │T1 │T2 │...│ │
              │ └───┴───┴───┘ │
              └───────────────┘
```

1. **Startup**: TPP calls `/api/login` N times with the same credential to acquire `pool_size` tokens
2. **Request**: When a client connects, TPP acquires a token from the pool (waits if all tokens are in use)
3. **Proxy**: TPP injects `Authorization: Bearer <token>` header and forwards the request
4. **Release**: When the connection closes, the token is returned to the pool
5. **Refresh**: Background task automatically refreshes tokens before TTL expires

## Health Check Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Full health status with pool info |
| `GET /healthz` | Same as `/health` |
| `GET /livez` | Liveness probe (always returns 200) |
| `GET /readyz` | Readiness probe (200 if pool has tokens) |
| `GET /metrics` | Prometheus format metrics |

### Example Response

```json
{
  "status": "healthy",
  "pool": {
    "total": 200,
    "in_use": 150,
    "available": 50,
    "waiting": 0
  }
}
```

## Metrics

| Metric | Description |
|--------|-------------|
| `tpp_tokens_total` | Total number of tokens in the pool |
| `tpp_tokens_in_use` | Number of tokens currently in use |
| `tpp_tokens_available` | Number of available tokens |
| `tpp_requests_waiting` | Number of requests waiting for a token |

## Docker Compose Example

```yaml
version: '3.8'

services:
  tpp:
    image: sinotrade/tpp:latest
    ports:
      - "8080:8080"
      - "9090:9090"
    volumes:
      - ./config.yaml:/app/config.yaml:ro
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9090/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3
    restart: unless-stopped
```

## Environment Variables

All configuration values can be overridden via environment variables. Environment variables take precedence over config file values.

| Variable | Description | Example |
|----------|-------------|---------|
| `TPP_LISTEN` | Proxy listen address | `0.0.0.0:8080` |
| `TPP_HEALTH_LISTEN` | Health check server address | `0.0.0.0:9090` |
| `TPP_UPSTREAM_HOST` | Upstream DolphinDB host | `dolphindb.example.com` |
| `TPP_UPSTREAM_PORT` | Upstream DolphinDB port | `8848` |
| `TPP_UPSTREAM_TLS` | Enable TLS for upstream | `true` or `1` |
| `TPP_CREDENTIAL_USERNAME` | DolphinDB username | `admin` |
| `TPP_CREDENTIAL_PASSWORD` | DolphinDB password | `secret` |
| `TPP_TOKEN_POOL_SIZE` | Number of tokens to acquire | `200` |
| `TPP_TOKEN_TTL_SECONDS` | Token TTL in seconds | `3600` |
| `TPP_TOKEN_REFRESH_CHECK_SECONDS` | Refresh check interval | `60` |
| `TPP_TELEMETRY_OTLP_ENDPOINT` | OTLP endpoint | `http://localhost:4317` |
| `TPP_TELEMETRY_LOG_FILTER` | Log level filter | `info`, `debug`, `tpp=debug` |

## Building from Source

### Prerequisites

- Rust 1.82.0 or later
- CMake

### Build

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test
```

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
