# Token Pool HTTP Proxy (tpp)

## 專案概述

基於 `pingora-proxy` 的 HTTP 反向代理，為 DolphinDB REST API 實現 Bearer Token 連線池功能：

- **自動申請 Token**：啟動時用 N 組帳號密碼呼叫 `/api/login` 取得 tokens
- **Per-Connection Token**：每個 TCP 連線綁定一個 token，連線期間獨佔
- **無限等待**：token 用完時，新連線等待直到有 token 釋放
- **自動刷新**：Token 過期自動重新登入刷新
- **健康檢查**：內建 `/health`, `/metrics` endpoints
- **OpenTelemetry**：完整的可觀測性支援

---

## 專案結構

```
tpp/
├── Cargo.toml
├── Dockerfile
├── config.example.yaml
├── PLAN.md
└── src/
    ├── main.rs              # CLI 入口、Server 初始化
    ├── lib.rs               # re-exports
    ├── proxy.rs             # ProxyHttp trait 實作
    ├── token_pool.rs        # Token Pool 核心
    ├── token_acquirer.rs    # 自動登入取得 token
    ├── token_refresher.rs   # 後台 token 刷新任務
    ├── health.rs            # 健康檢查 HTTP server
    ├── config.rs            # 設定檔解析
    ├── telemetry.rs         # OpenTelemetry 整合
    └── error.rs             # 錯誤處理
```

---

## 運作流程

```
1. 讀取設定檔（credentials）
2. 對每組帳密呼叫 DolphinDB /api/login 取得 token
3. 建立 TokenPool（使用 async_channel 實現 semaphore）
4. 啟動後台 Token 刷新任務
5. 啟動健康檢查 HTTP server
6. 啟動 HTTP Proxy

[Client] → [tpp:8080] → [DolphinDB:8848]
                ↓
         注入 Authorization: Bearer <token>
```

---

## 設定檔格式

```yaml
listen: "0.0.0.0:8080"
health_listen: "0.0.0.0:9090"  # 健康檢查 endpoint

upstream:
  host: "dolphindb.example.com"
  port: 8848
  tls: false

token:
  ttl_seconds: 3600           # Token 有效期（預設 1 小時）
  refresh_check_seconds: 60   # 刷新檢查間隔

credentials:
  - username: "user1"
    password: "pass1"
  - username: "user2"
    password: "pass2"

# 或從檔案載入（每行 username:password）
# credentials_file: "/path/to/credentials.txt"

telemetry:
  otlp_endpoint: "http://localhost:4317"  # optional
  log_filter: "info"
```

---

## 使用方式

```bash
# 編譯
cargo build --release

# 執行
./target/release/tpp --config config.yaml

# Docker
docker build -t tpp .
docker run -v $(pwd)/config.yaml:/app/config.yaml -p 8080:8080 -p 9090:9090 tpp
```

---

## 健康檢查 Endpoints

| Endpoint | 說明 |
|----------|------|
| `GET /health` | 完整健康狀態 + pool 資訊 |
| `GET /healthz` | 同 `/health` |
| `GET /livez` | Liveness probe（總是 200） |
| `GET /readyz` | Readiness probe（pool 有 token 時 200） |
| `GET /metrics` | Prometheus 格式的 metrics |

### /health 回應範例

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

---

## Token 刷新機制

1. **定時刷新**：後台任務每 `refresh_check_seconds` 秒檢查一次
2. **TTL 過期**：Token 超過 `ttl_seconds` 秒自動刷新
3. **錯誤觸發**：收到 401 時標記 token 需要刷新
4. **無縫更新**：刷新時不影響正在使用的連線

---

## Metrics

| Metric | 說明 |
|--------|------|
| `tpp_tokens_total` | 總 token 數 |
| `tpp_tokens_in_use` | 使用中 token 數 |
| `tpp_tokens_available` | 可用 token 數 |
| `tpp_requests_waiting` | 等待中請求數 |

---

## 核心元件

### TokenPool
- 使用 `async_channel` bounded channel 實現 semaphore 語義
- 每個 token 包含 credential 資訊（供刷新使用）
- 追蹤每個 token 的使用次數、錯誤次數、最後使用時間

### TokenRefresher
- 後台 tokio task
- 定期檢查過期 token
- 監聽 Notify 即時刷新標記的 token

### Health Server
- 獨立的 axum HTTP server
- 支援 Kubernetes liveness/readiness probes
- Prometheus 格式 metrics
