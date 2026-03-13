use anyhow::{Context, Result};
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace as sdktrace};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(otlp_endpoint: &str, service_name: &str) -> Result<()> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint),
        )
        .with_trace_config(
            sdktrace::config().with_resource(opentelemetry_sdk::Resource::new(vec![
                opentelemetry::KeyValue::new("service.name", service_name.to_string()),
            ])),
        )
        .install_batch(runtime::Tokio)
        .context("failed to install OTLP tracer")?;

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    Ok(())
}

pub fn shutdown() {
    global::shutdown_tracer_provider();
}
