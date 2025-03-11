use std::{env, time::Duration};

use opentelemetry::{trace::TraceError, KeyValue};
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, ResourceDetector, TelemetryResourceDetector},
    runtime,
    trace::{BatchConfig, Config, Tracer},
    Resource,
};
use opentelemetry_semantic_conventions::{
    resource::{SERVICE_NAME, SERVICE_VERSION},
    SCHEMA_URL,
};
use sentry_tracing::EventFilter;
use tracing::{level_filters::LevelFilter, Subscriber};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{registry::LookupSpan, EnvFilter, Layer};

fn otlp_tracer(keypairs: &[KeyValue]) -> Result<Tracer, TraceError> {
    let noop_timeout = Duration::from_secs(0);
    let telemetry = TelemetryResourceDetector.detect(noop_timeout);
    let mut env = EnvResourceDetector::new().detect(noop_timeout);
    let mut resource = Resource::from_schema_url(keypairs.to_vec(), SCHEMA_URL);

    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_trace_config(
            Config::default().with_resource(telemetry.merge(&mut env).merge(&mut resource)),
        )
        .with_batch_config(BatchConfig::default())
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .install_batch(runtime::Tokio)
}

pub fn sentry<S>() -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    sentry_tracing::layer()
        .enable_span_attributes()
        .event_filter(|md| match md.level() {
            &tracing::Level::ERROR => EventFilter::Event,
            _ => EventFilter::Ignore,
        })
}

pub fn logging<S>() -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_filter(EnvFilter::from_default_env())
}

pub fn open_telemetry<S>(
    service_name: &'static str,
    service_version: &'static str,
) -> Result<impl Layer<S>, TraceError>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let tracer = otlp_tracer(&[
        KeyValue::new(SERVICE_NAME, service_name),
        KeyValue::new(SERVICE_VERSION, service_version),
    ])?;

    Ok(OpenTelemetryLayer::new(tracer).with_filter(
        if env::var("OTEL_SDK_DISABLED").ok().is_some() {
            tracing::info!("disabling opentelemetry as per OTEL_SDK_DISABLED");
            LevelFilter::OFF
        } else {
            LevelFilter::INFO
        },
    ))
}
