use opentelemetry::global;
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter, MeterProvider};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_sdk::{runtime, Resource};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "tpp";

static METRICS: OnceLock<PoolMetrics> = OnceLock::new();
static OTEL_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Metrics for token pool proxy
pub struct PoolMetrics {
    // Pool state gauges
    pub tokens_total: Gauge<u64>,
    pub tokens_in_use: Gauge<u64>,
    pub tokens_available: Gauge<u64>,
    pub requests_waiting: Gauge<u64>,

    // Operation counters
    pub token_acquisitions: Counter<u64>,
    pub token_releases: Counter<u64>,
    pub token_errors: Counter<u64>,

    // Latency histograms
    pub acquisition_wait_seconds: Histogram<f64>,
    pub connection_duration_seconds: Histogram<f64>,

    // HTTP metrics
    pub requests_total: Counter<u64>,
    pub requests_errors: Counter<u64>,
}

impl PoolMetrics {
    fn new(meter: &Meter) -> Self {
        Self {
            tokens_total: meter
                .u64_gauge("tpp_tokens_total")
                .with_description("Total number of tokens in the pool")
                .build(),
            tokens_in_use: meter
                .u64_gauge("tpp_tokens_in_use")
                .with_description("Number of tokens currently in use")
                .build(),
            tokens_available: meter
                .u64_gauge("tpp_tokens_available")
                .with_description("Number of tokens available for use")
                .build(),
            requests_waiting: meter
                .u64_gauge("tpp_requests_waiting")
                .with_description("Number of requests waiting for a token")
                .build(),
            token_acquisitions: meter
                .u64_counter("tpp_token_acquisitions_total")
                .with_description("Total number of token acquisitions")
                .build(),
            token_releases: meter
                .u64_counter("tpp_token_releases_total")
                .with_description("Total number of token releases")
                .build(),
            token_errors: meter
                .u64_counter("tpp_token_errors_total")
                .with_description("Total number of token errors")
                .build(),
            acquisition_wait_seconds: meter
                .f64_histogram("tpp_acquisition_wait_seconds")
                .with_description("Time spent waiting to acquire a token")
                .build(),
            connection_duration_seconds: meter
                .f64_histogram("tpp_connection_duration_seconds")
                .with_description("Connection duration in seconds")
                .build(),
            requests_total: meter
                .u64_counter("tpp_requests_total")
                .with_description("Total number of HTTP requests proxied")
                .build(),
            requests_errors: meter
                .u64_counter("tpp_requests_errors_total")
                .with_description("Total number of failed HTTP requests")
                .build(),
        }
    }
}

pub fn get_metrics() -> Option<&'static PoolMetrics> {
    METRICS.get()
}

pub struct TelemetryConfig {
    pub otlp_endpoint: Option<String>,
    pub log_filter: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: None,
            log_filter: "info".to_string(),
        }
    }
}

fn create_resource() -> Resource {
    Resource::new(vec![
        KeyValue::new(
            opentelemetry_semantic_conventions::attribute::SERVICE_NAME,
            SERVICE_NAME,
        ),
        KeyValue::new(
            opentelemetry_semantic_conventions::attribute::SERVICE_VERSION,
            env!("CARGO_PKG_VERSION"),
        ),
    ])
}

fn init_tracer_provider(
    endpoint: &str,
) -> Result<TracerProvider, opentelemetry::trace::TraceError> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(3))
        .build()?;

    let provider = TracerProvider::builder()
        .with_resource(create_resource())
        .with_batch_exporter(exporter, runtime::Tokio)
        .build();

    Ok(provider)
}

fn init_meter_provider(
    endpoint: &str,
) -> Result<SdkMeterProvider, opentelemetry_sdk::metrics::MetricError> {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(3))
        .build()?;

    let reader = PeriodicReader::builder(exporter, runtime::Tokio)
        .with_interval(Duration::from_secs(10))
        .build();

    let provider = SdkMeterProvider::builder()
        .with_resource(create_resource())
        .with_reader(reader)
        .build();

    Ok(provider)
}

pub fn init_telemetry(config: TelemetryConfig) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::new(&config.log_filter);

    match &config.otlp_endpoint {
        Some(endpoint) => {
            // Create a dedicated tokio runtime for OpenTelemetry
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;

            // Initialize OpenTelemetry within the runtime context
            let endpoint_clone = endpoint.clone();
            let (tracer_provider, meter_provider) = rt.block_on(async {
                let tracer_provider = init_tracer_provider(&endpoint_clone)
                    .map_err(|e| format!("Failed to init tracer: {}", e))?;
                let meter_provider = init_meter_provider(&endpoint_clone)
                    .map_err(|e| format!("Failed to init meter: {}", e))?;
                Ok::<_, Box<dyn std::error::Error>>((tracer_provider, meter_provider))
            })?;

            // Store the runtime to keep it alive
            let _ = OTEL_RUNTIME.set(rt);

            // Set global providers
            global::set_tracer_provider(tracer_provider.clone());
            global::set_meter_provider(meter_provider.clone());

            // Create tracer for tracing-opentelemetry layer
            let tracer = tracer_provider.tracer(SERVICE_NAME);

            // Initialize metrics
            let meter = meter_provider.meter(SERVICE_NAME);
            let _ = METRICS.set(PoolMetrics::new(&meter));

            // Set up tracing subscriber with OpenTelemetry layer
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .with(otel_layer)
                .init();

            info!(
                endpoint = %endpoint,
                "OpenTelemetry initialized with OTLP export"
            );
        }
        None => {
            // Initialize without OpenTelemetry - just tracing-subscriber for console logging
            let meter_provider = SdkMeterProvider::builder()
                .with_resource(create_resource())
                .build();

            global::set_meter_provider(meter_provider.clone());

            let meter = meter_provider.meter(SERVICE_NAME);
            let _ = METRICS.set(PoolMetrics::new(&meter));

            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();

            info!("Telemetry initialized without OTLP export");
        }
    }

    Ok(())
}

pub fn shutdown_telemetry() {
    global::shutdown_tracer_provider();
}
