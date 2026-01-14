# TPP - Token Pool Proxy

[![CI](https://github.com/Yvictor/tpp/actions/workflows/ci.yml/badge.svg)](https://github.com/Yvictor/tpp/actions/workflows/ci.yml)
[![Release](https://github.com/Yvictor/tpp/actions/workflows/release.yml/badge.svg)](https://github.com/Yvictor/tpp/actions/workflows/release.yml)
[![Docker](https://img.shields.io/docker/v/yvictor/tpp?label=docker&sort=semver)](https://hub.docker.com/r/yvictor/tpp)

A high-performance HTTP reverse proxy built with [Pingora](https://github.com/cloudflare/pingora) that provides Bearer token connection pooling for DolphinDB REST API.

## Features

- **Automatic Token Acquisition** - Automatically acquires tokens by calling `/api/login` with configured credentials at startup
- **Per-Connection Token Binding** - Each TCP connection is bound to a dedicated token for the duration of the connection
- **Connection Queuing** - When all tokens are in use, new connections wait until a token becomes available
- **Auto Token Refresh** - Automatically refreshes tokens before they expire
- **Health Check Endpoints** - Built-in `/health`, `/livez`, `/readyz`, and `/metrics` endpoints
- **OpenTelemetry Support** - Full observability with traces and metrics export

## Quick Start

### Using Docker

```bash
docker run -d \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -p 8080:8080 \
  -p 9090:9090 \
  yvictor/tpp:latest
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

# Health check server (optional)
health_listen: "0.0.0.0:9090"

# Upstream DolphinDB server
upstream:
  host: "dolphindb.example.com"
  port: 8848
  tls: false

# Token refresh settings
token:
  ttl_seconds: 3600        # Token TTL (default: 1 hour)
  refresh_check_seconds: 60 # Refresh check interval

# User credentials for token acquisition
credentials:
  - username: "user1"
    password: "password1"
  - username: "user2"
    password: "password2"

# Or load from file (format: username:password per line)
# credentials_file: "/path/to/credentials.txt"

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

1. **Startup**: TPP calls `/api/login` for each credential to acquire tokens
2. **Request**: When a client connects, TPP acquires a token from the pool
3. **Proxy**: TPP injects `Authorization: Bearer <token>` header and forwards the request
4. **Release**: When the connection closes, the token is returned to the pool
5. **Refresh**: Background task automatically refreshes expiring tokens

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
    image: yvictor/tpp:latest
    ports:
      - "8080:8080"
      - "9090:9090"
    volumes:
      - ./config.yaml:/app/config.yaml:ro
      - ./credentials.txt:/app/credentials.txt:ro
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9090/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3
    restart: unless-stopped
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level filter (e.g., `info`, `debug`, `tpp=debug`) |

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
