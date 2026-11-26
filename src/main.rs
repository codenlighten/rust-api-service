use std::future::IntoFuture;
use std::sync::Arc;

use anyhow::Context;
use notes_api::app::{router, AppState};
use notes_api::config::Config;
use notes_api::{db, password::Hasher};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
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

    let (signalled, signal_seen) = oneshot::channel();
    let server = axum::serve(listener, router(state)).with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = signalled.send(());
    });
    // Draining waits for every request, and a client that never finishes sending one would hold
    // the process open for ever. After the signal there is a deadline.
    tokio::select! {
        result = server.into_future() => result.context("server error")?,
        () = async {
            let _ = signal_seen.await;
            tokio::time::sleep(config.shutdown_timeout).await;
        } => anyhow::bail!(
            "requests were still running {}s after the shutdown signal; abandoning them",
            config.shutdown_timeout.as_secs()
        ),
    }

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
