use tonic_tracing_opentelemetry::middleware::{client, server};

pub fn server_layer() -> server::OtelGrpcLayer {
    server::OtelGrpcLayer::default()
}

pub fn client_layer() -> client::OtelGrpcLayer {
    client::OtelGrpcLayer
}
