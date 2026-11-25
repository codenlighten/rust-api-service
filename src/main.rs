use std::sync::Arc;

use anyhow::Context;
use notes_api::app::{router, AppState};
use notes_api::config::Config;
use notes_api::{db, password::Hasher};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    init_tracing(config.json_logs);

    let pool = db::connect(&config.database_url, config.db_max_connections)
        .await
        .with_context(|| format!("could not open database '{}'", config.database_url))?;

    let state = AppState {
        pool: pool.clone(),
        hasher: Arc::new(Hasher::standard()),
        token_secret: config.token_secret.clone().into(),
        token_ttl_secs: config.token_ttl_secs,
    };

    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("could not bind {}", config.bind_addr))?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    // In-flight requests have finished; flush and close the database.
    pool.close().await;
    tracing::info!("shutdown complete");
    Ok(())
}

fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.init();
    }
}

/// Resolves on Ctrl-C or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received; draining in-flight requests");
}
