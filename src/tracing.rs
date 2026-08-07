use std::env;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::ExporterBuildError;
use opentelemetry_sdk::{
    Resource,
    resource::{EnvResourceDetector, TelemetryResourceDetector},
    trace::SdkTracerProvider,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use tracing::{Subscriber, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, Layer, registry::LookupSpan};

use crate::json_fields::{ErrorJsonFields, ErrorJsonFormat};

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

/// Creates a JSON logging layer that includes typed error source chains.
///
/// Source capture requires recording errors as typed values, for example:
///
/// ```text
/// error = &error as &(dyn std::error::Error + 'static)
/// ```
///
/// `error = %error` and `error = ?error` record only formatted text. Likewise,
/// `#[instrument(err(Debug))]` can record only the returned error; after a domain
/// error is converted to a transport error such as `tonic::Status`, its original
/// sources cannot be reconstructed.
pub fn logging<S>() -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    tracing_subscriber::fmt::layer()
        .fmt_fields(ErrorJsonFields)
        .event_format(ErrorJsonFormat)
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
        .with_error_events_to_exceptions(true)
        .with_error_records_to_exceptions(true)
        .with_filter(if env::var("OTEL_SDK_DISABLED").ok().is_some() {
            tracing::info!("disabling opentelemetry as per OTEL_SDK_DISABLED");
            LevelFilter::OFF
        } else {
            LevelFilter::INFO
        }))
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        fmt::{self, Display},
        io::Write,
        sync::{Arc, Mutex},
    };

    use serde_json::{Value, json};
    use tracing::subscriber::with_default;
    use tracing_subscriber::{
        Registry,
        fmt::{self as tracing_fmt, MakeWriter},
        layer::SubscriberExt,
    };

    use crate::json_fields::{ErrorJsonFields, ErrorJsonFormat};

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    struct BufferWriter(Buffer);

    impl Write for BufferWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.0.lock().unwrap().write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.0.lock().unwrap().flush()
        }
    }

    impl<'writer> MakeWriter<'writer> for Buffer {
        type Writer = BufferWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            BufferWriter(self.clone())
        }
    }

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<dyn Error + Send + Sync>>,
    }

    impl TestError {
        fn new(message: &'static str) -> Self {
            Self {
                message,
                source: None,
            }
        }

        fn with_source(message: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
            Self {
                message,
                source: Some(Box::new(source)),
            }
        }
    }

    impl Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl Error for TestError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source.as_deref().map(|source| source as _)
        }
    }

    #[derive(Debug)]
    struct DebugField {
        id: u8,
    }

    fn subscriber(buffer: Buffer) -> impl tracing::Subscriber {
        Registry::default().with(
            tracing_fmt::layer()
                .fmt_fields(ErrorJsonFields)
                .event_format(ErrorJsonFormat)
                .with_writer(buffer),
        )
    }

    fn events(buffer: &Buffer) -> Vec<Value> {
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn records_nested_typed_error_sources_in_one_event() {
        let buffer = Buffer::default();
        let subscriber = subscriber(buffer.clone());
        let error = TestError::with_source(
            "outer error",
            TestError::with_source(
                "intermediate error",
                std::io::Error::other("root I/O error"),
            ),
        );

        with_default(subscriber, || {
            tracing::error!(error = &error as &(dyn Error + 'static), "operation failed");
        });

        let events = events(&buffer);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["error"], "outer error");
        assert_eq!(
            events[0]["error.chain"],
            json!(["intermediate error", "root I/O error"])
        );
    }

    #[test]
    fn records_realistic_wrapped_error() {
        let buffer = Buffer::default();
        let subscriber = subscriber(buffer.clone());
        let error = TestError::with_source(
            "secret storage operation failed",
            TestError::new("OpenBao denied the operation"),
        );

        with_default(subscriber, || {
            tracing::error!(
                error = &error as &(dyn Error + 'static),
                "gRPC error occurred"
            );
        });

        let event = events(&buffer).pop().unwrap();
        assert_eq!(event["error"], "secret storage operation failed");
        assert_eq!(
            event["error.chain"],
            json!(["OpenBao denied the operation"])
        );
    }

    #[test]
    fn records_an_empty_chain_for_an_error_without_a_source() {
        let buffer = Buffer::default();
        let subscriber = subscriber(buffer.clone());
        let error = TestError::new("standalone error");

        with_default(subscriber, || {
            tracing::error!(error = &error as &(dyn Error + 'static), "operation failed");
        });

        let event = events(&buffer).pop().unwrap();
        assert_eq!(event["error"], "standalone error");
        assert_eq!(event["error.chain"], json!([]));
    }

    #[test]
    fn preserves_ordinary_json_fields() {
        let buffer = Buffer::default();
        let subscriber = subscriber(buffer.clone());

        let debug_value = DebugField { id: 7 };
        assert_eq!(debug_value.id, 7);
        with_default(subscriber, || {
            tracing::info!(
                string = "value",
                integer = 42,
                boolean = true,
                debug = ?debug_value,
                "ordinary fields"
            );
        });

        let event = events(&buffer).pop().unwrap();
        assert_eq!(event["message"], "ordinary fields");
        assert_eq!(event["string"], "value");
        assert_eq!(event["integer"], 42);
        assert_eq!(event["boolean"], true);
        assert_eq!(event["debug"], "DebugField { id: 7 }");
    }

    #[test]
    fn records_error_fields_on_spans() {
        let buffer = Buffer::default();
        let subscriber = subscriber(buffer.clone());
        let error = TestError::with_source("span error", TestError::new("span source"));

        with_default(subscriber, || {
            let span = tracing::info_span!("request", error = tracing::field::Empty);
            span.record("error", &error as &(dyn Error + 'static));
            let _guard = span.enter();
            tracing::info!("handled request");
        });

        let event = events(&buffer).pop().unwrap();
        for span in [
            &event["span"],
            event["spans"].as_array().unwrap().last().unwrap(),
        ] {
            assert_eq!(span["error"], "span error");
            assert_eq!(span["error.chain"], json!(["span source"]));
        }
    }

    #[test]
    fn merges_late_span_records_as_valid_json() {
        let buffer = Buffer::default();
        let subscriber = subscriber(buffer.clone());
        let error = TestError::with_source("late error", TestError::new("late source"));

        with_default(subscriber, || {
            let span =
                tracing::info_span!("request", request_id = 42, error = tracing::field::Empty);
            span.record("error", &error as &(dyn Error + 'static));
            let _guard = span.enter();
            tracing::info!("handled request");
        });

        let event = events(&buffer).pop().unwrap();
        assert_eq!(event["span"]["request_id"], 42);
        assert_eq!(event["span"]["error"], "late error");
        assert_eq!(event["span"]["error.chain"], json!(["late source"]));
    }
}
