use once_cell::sync::Lazy;
use tracing_subscriber::{fmt, EnvFilter};

static LOG_INIT: Lazy<()> = Lazy::new(|| {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,smart_relay=debug"));
    fmt().with_target(false).with_env_filter(filter).init();
});

pub fn install_tracing() {
    Lazy::force(&LOG_INIT);
}

