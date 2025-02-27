use std::{borrow::Cow, env};

pub mod axum;
pub mod tonic;
mod tracing;

pub use tracing::*;

pub fn sentry_init() -> sentry::ClientInitGuard {
    sentry::init(sentry::ClientOptions {
        dsn: env::var("SENTRY_DSN").ok().and_then(|dsn| dsn.parse().ok()),
        environment: env::var("ENVIRONMENT").map(Cow::Owned).ok(),
        traces_sample_rate: 0.0,
        release: sentry::release_name!(),
        attach_stacktrace: true,
        ..sentry::ClientOptions::default()
    })
}
