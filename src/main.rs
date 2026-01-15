#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use std::path::PathBuf;
use std::process;
use std::time::Duration;

use clap::Parser;
use pingora::prelude::*;
use pingora_proxy::http_proxy_service;
use tracing::{error, info};

use tpp::config::Config;
use tpp::proxy::TokenPoolProxy;
use tpp::telemetry::{init_telemetry, TelemetryConfig};
use tpp::token_acquirer::TokenAcquirer;
use tpp::token_pool::TokenPool;

#[derive(Parser, Debug)]
#[command(name = "tpp")]
#[command(about = "Token Pool HTTP Proxy - Bearer token connection pooling for DolphinDB")]
#[command(version)]
struct Args {
    /// Path to configuration file (optional, can use env vars instead)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    // Load configuration from file or environment variables
    let config = match args.config {
        Some(path) => match Config::from_file(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to load config: {}", e);
                process::exit(1);
            }
        },
        None => match Config::from_env() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to load config from env: {}", e);
                process::exit(1);
            }
        },
    };

    // Initialize telemetry
    let telemetry_config = TelemetryConfig {
        otlp_endpoint: config.telemetry.otlp_endpoint.clone(),
        log_filter: config
            .telemetry
            .log_filter
            .clone()
            .unwrap_or_else(|| "info".to_string()),
    };
    if let Err(e) = init_telemetry(telemetry_config) {
        eprintln!("Failed to initialize telemetry: {}", e);
        process::exit(1);
    }

    info!(
        "Credential: user='{}', pool_size={}",
        config.credential.username, config.token.pool_size
    );

    // Use a dedicated runtime for async initialization (token acquisition)
    // This runtime will be dropped before Pingora creates its own
    let (pool, acquirer) = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for initialization");

        rt.block_on(async {
            let acquirer = TokenAcquirer::new(&config.upstream.base_url());
            let tokens = match acquirer
                .acquire_n(&config.credential, config.token.pool_size)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    error!("Failed to acquire tokens: {}", e);
                    process::exit(1);
                }
            };

            info!("Acquired {} tokens", tokens.len());

            let pool = TokenPool::new(tokens, config.credential.clone());
            (pool, acquirer)
        })
    };

    // Start health check server and token refresher on Pingora's runtime
    let health_addr = config.health_listen.clone();
    let ttl = Duration::from_secs(config.token.ttl_seconds);
    let check_interval = Duration::from_secs(config.token.refresh_check_seconds);
    let pool_for_health = pool.clone();
    let pool_for_refresher = pool.clone();

    // Create proxy
    let proxy = TokenPoolProxy::new(pool.clone(), config.upstream.address(), config.upstream.tls);

    // Create Pingora server
    let mut server = match Server::new(Some(Opt::default())) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create server: {}", e);
            process::exit(1);
        }
    };
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

    // Spawn health server and refresher after Pingora starts
    // Use a background service to spawn these tasks
    std::thread::spawn(move || {
        // Wait a bit for Pingora to fully start
        std::thread::sleep(Duration::from_millis(100));

        // Create a runtime for background tasks
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create background runtime");

        rt.block_on(async {
            // Start health check server if configured
            if let Some(addr) = health_addr {
                tpp::health::spawn_health_server(addr.clone(), pool_for_health);
                info!("Health check server started on {}", addr);
            }

            // Start token refresher
            tpp::token_refresher::spawn_refresher(
                pool_for_refresher,
                acquirer,
                ttl,
                check_interval,
            );
            info!(
                "Token refresher started (TTL: {}s, check interval: {}s)",
                ttl.as_secs(),
                check_interval.as_secs()
            );

            // Keep the runtime alive
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });
    });

    server.run_forever();
}
