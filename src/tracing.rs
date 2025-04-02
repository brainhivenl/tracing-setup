use std::env;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::ExporterBuildError;
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, TelemetryResourceDetector},
    trace::SdkTracerProvider,
    Resource,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use sentry_tracing::EventFilter;
use tracing::{level_filters::LevelFilter, Subscriber};
use tracing_subscriber::{registry::LookupSpan, EnvFilter, Layer};

fn sdk_provider(keypairs: &[KeyValue]) -> Result<SdkTracerProvider, ExporterBuildError> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()?;

    Ok(opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_detectors(&[
                    Box::new(EnvResourceDetector::new()),
                    Box::new(TelemetryResourceDetector),
                ])
                .with_attributes(keypairs.to_vec())
                .build(),
        )
        .with_batch_exporter(exporter)
        .build())
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
) -> Result<impl Layer<S>, ExporterBuildError>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let provider = sdk_provider(&[
        KeyValue::new(SERVICE_NAME, service_name),
        KeyValue::new(SERVICE_VERSION, service_version),
    ])?;

    Ok(tracing_opentelemetry::layer()
        .with_tracer(provider.tracer(service_name))
        .with_filter(if env::var("OTEL_SDK_DISABLED").ok().is_some() {
            tracing::info!("disabling opentelemetry as per OTEL_SDK_DISABLED");
            LevelFilter::OFF
        } else {
            LevelFilter::INFO
        }))
}
