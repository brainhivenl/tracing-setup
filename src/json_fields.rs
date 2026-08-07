use std::{collections::BTreeMap, error::Error, fmt};

use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::Record,
};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    field::RecordFields,
    fmt::{
        FmtContext, FormattedFields,
        format::{FormatEvent, FormatFields, Writer},
        time::{FormatTime, SystemTime},
    },
    registry::{LookupSpan, SpanRef},
};

#[derive(Debug, Default)]
pub struct ErrorJsonFields;

impl<'writer> FormatFields<'writer> for ErrorJsonFields {
    fn format_fields<R: RecordFields>(&self, mut writer: Writer<'_>, fields: R) -> fmt::Result {
        let mut visitor = ErrorJsonVisitor::default();
        fields.record(&mut visitor);
        visitor.write_to(&mut writer)
    }

    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &Record<'_>,
    ) -> fmt::Result {
        let mut visitor = if current.is_empty() {
            ErrorJsonVisitor::default()
        } else {
            ErrorJsonVisitor {
                values: serde_json::from_str(current).map_err(|_| fmt::Error)?,
            }
        };

        fields.record(&mut visitor);
        current.fields.clear();
        visitor.write_to(&mut current.as_writer())
    }
}

#[derive(Default)]
struct ErrorJsonVisitor {
    values: BTreeMap<String, serde_json::Value>,
}

impl ErrorJsonVisitor {
    fn insert(&mut self, field: &Field, value: impl Into<serde_json::Value>) {
        self.values.insert(field.name().to_owned(), value.into());
    }

    fn write_to(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        let fields = serde_json::to_string(&self.values).map_err(|_| fmt::Error)?;
        writer.write_str(&fields)
    }
}

#[derive(Debug, Default)]
pub struct ErrorJsonFormat;

impl<S> FormatEvent<S, ErrorJsonFields> for ErrorJsonFormat
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, ErrorJsonFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut timestamp = String::new();
        SystemTime.format_time(&mut Writer::new(&mut timestamp))?;

        let mut event_fields = ErrorJsonVisitor::default();
        event.record(&mut event_fields);

        let normalized_metadata = event.normalized_metadata();
        let metadata = normalized_metadata
            .as_ref()
            .unwrap_or_else(|| event.metadata());
        let mut output = serde_json::Map::new();
        output.insert("timestamp".to_owned(), timestamp.into());
        output.insert("level".to_owned(), metadata.level().as_str().into());
        output.extend(event_fields.values);
        output.insert("target".to_owned(), metadata.target().into());

        let current_span = event
            .parent()
            .and_then(|id| ctx.span(id))
            .or_else(|| ctx.lookup_current());

        if let Some(span) = current_span {
            output.insert("span".to_owned(), span_to_json(&span)?);
            output.insert(
                "spans".to_owned(),
                ctx.lookup_current()
                    .map(|span| {
                        span.scope()
                            .from_root()
                            .map(|span| span_to_json(&span))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default()
                    .into(),
            );
        }

        let output = serde_json::to_string(&output).map_err(|_| fmt::Error)?;
        writeln!(writer, "{output}")
    }
}

fn span_to_json<S>(span: &SpanRef<'_, S>) -> Result<serde_json::Value, fmt::Error>
where
    S: for<'lookup> LookupSpan<'lookup>,
{
    let extensions = span.extensions();
    let fields = extensions
        .get::<FormattedFields<ErrorJsonFields>>()
        .expect("span fields must be formatted before they are serialized");
    let mut fields = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(fields)
        .map_err(|_| fmt::Error)?;
    fields.insert("name".to_owned(), span.metadata().name().into());

    Ok(fields.into())
}

impl Visit for ErrorJsonVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.insert(field, value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, value);
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        self.insert(field, value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();

        if name.starts_with("log.") {
            return;
        }

        self.values.insert(
            name.strip_prefix("r#").unwrap_or(name).to_owned(),
            format!("{value:?}").into(),
        );
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        self.insert(field, value.to_string());

        let mut chain = Vec::new();
        let mut source = value.source();
        while let Some(error) = source {
            chain.push(error.to_string());
            source = error.source();
        }

        self.values.insert(
            format!("{}.chain", field.name()),
            serde_json::Value::from(chain),
        );
    }
}
