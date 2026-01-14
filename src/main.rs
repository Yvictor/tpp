#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use pingora::prelude::*;
use pingora_proxy::http_proxy_service;
use tracing::info;

use tpp::config::Config;
use tpp::health::spawn_health_server;
use tpp::proxy::TokenPoolProxy;
use tpp::telemetry::{init_telemetry, TelemetryConfig};
use tpp::token_acquirer::TokenAcquirer;
use tpp::token_pool::TokenPool;
use tpp::token_refresher::spawn_refresher;

#[derive(Parser, Debug)]
#[command(name = "tpp")]
#[command(about = "Token Pool HTTP Proxy - Bearer token connection pooling for DolphinDB")]
#[command(version)]
struct Args {
    /// Path to configuration file
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Load configuration
    let config = Config::from_file(&args.config)?;

    // Initialize telemetry
    let telemetry_config = TelemetryConfig {
        otlp_endpoint: config.telemetry.otlp_endpoint.clone(),
        log_filter: config
            .telemetry
            .log_filter
            .clone()
            .unwrap_or_else(|| "info".to_string()),
    };
    init_telemetry(telemetry_config)?;

    // Load credentials
    let credentials = config.load_credentials()?;
    info!("Loaded {} credentials", credentials.len());

    // Acquire tokens from DolphinDB
    let acquirer = TokenAcquirer::new(&config.upstream.base_url());
    let tokens_with_creds = acquirer.acquire_all(&credentials).await?;

    info!("Created token pool with {} tokens", tokens_with_creds.len());

    // Create token pool
    let pool = TokenPool::new(tokens_with_creds);

    // Start health check server if configured
    if let Some(health_addr) = &config.health_listen {
        spawn_health_server(health_addr.clone(), pool.clone());
        info!("Health check server started on {}", health_addr);
    }

    // Start token refresher
    let ttl = Duration::from_secs(config.token.ttl_seconds);
    let check_interval = Duration::from_secs(config.token.refresh_check_seconds);
    spawn_refresher(pool.clone(), acquirer.clone(), ttl, check_interval);
    info!(
        "Token refresher started (TTL: {}s, check interval: {}s)",
        config.token.ttl_seconds, config.token.refresh_check_seconds
    );

    // Create proxy
    let proxy = TokenPoolProxy::new(
        pool.clone(),
        config.upstream.address(),
        config.upstream.tls,
    );

    // Create Pingora server
    let mut server = Server::new(Some(Opt::default()))?;
    server.bootstrap();

    // Create HTTP proxy service
    let mut proxy_service = http_proxy_service(&server.configuration, proxy);
    proxy_service.add_tcp(&config.listen);

    info!(
        listen = %config.listen,
        upstream = %config.upstream.address(),
        tls = config.upstream.tls,
        pool_size = pool.total(),
        "Starting Token Pool Proxy"
    );

    server.add_service(proxy_service);
    server.run_forever();
}
