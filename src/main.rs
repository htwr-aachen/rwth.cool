use rwth_cool::{app, build_state};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Base URL used for canonical links when `BASE_URL` is not set.
const DEFAULT_BASE_URL: &str = "https://rwth.cool";
/// Port used for serving and for the health check when `PORT` is not set.
const DEFAULT_PORT: u16 = 3000;

#[tokio::main]
async fn main() {
    // `./rwth_cool --healthcheck` is used by the Docker health check: it does
    // not start the server, but probes the running one and exits.
    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        std::process::exit(healthcheck());
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let base_url = std::env::var("BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let config_path =
        std::env::var("REDIRECTS_FILE").unwrap_or_else(|_| "redirects.toml".to_string());
    let config = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("failed to read {config_path}: {err}"));
    let state = build_state(&config, &base_url)
        .unwrap_or_else(|err| panic!("invalid configuration in {config_path}: {err}"));

    let port = port();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    tracing::info!(%addr, %base_url, config = %config_path, "listening");

    axum::serve(listener, app(Arc::new(state)))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// Returns the configured port.
fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Resolves on SIGINT (Ctrl+C) or SIGTERM, so that in-flight requests can finish.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}

/// Probes the local `/healthz` endpoint and returns a process exit code.
fn healthcheck() -> i32 {
    use std::io::{Read, Write};

    let result = (|| -> std::io::Result<bool> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port()))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream
            .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200"))
    })();

    match result {
        Ok(true) => 0,
        Ok(false) => {
            eprintln!("health check failed: unexpected response");
            1
        }
        Err(err) => {
            eprintln!("health check failed: {err}");
            1
        }
    }
}
