use tracing_subscriber::{fmt, EnvFilter};

pub fn init(log_level: &str) {
    let filter =
        EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("gateway_api=info"));
    let _ = fmt().with_env_filter(filter).json().try_init();
}
