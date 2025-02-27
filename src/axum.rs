use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};

pub fn in_response_layer() -> OtelInResponseLayer {
    OtelInResponseLayer
}

pub fn layer() -> OtelAxumLayer {
    OtelAxumLayer::default()
}
