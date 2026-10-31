use inotify::{api, bootstrap_admin, config::Config, db, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env();
    let pool = db::connect(&config.data_dir).await?;
    if let Some(password) = bootstrap_admin(&pool).await? {
        tracing::warn!("created administrator: username `admin`, password `{password}` (shown only once, change it after logging in)");
    }

    let state = AppState::new(pool, &config).await?;
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!("listening on {}", config.listen);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}
