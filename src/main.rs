use axum::{
    extract::{Host, Path},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Redirect},
    routing::get,
    Router,
};
use askama::Template;
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Deserialize)]
struct Config {
    redirects: HashMap<String, String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    redirects: &'a HashMap<String, String>,
}

// Custom response type to handle both redirects and template rendering
enum AppResponse {
    Redirect(Redirect),
    Template(IndexTemplate<'static>),
    NotFound(String),
}

impl IntoResponse for AppResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            AppResponse::Redirect(redirect) => redirect.into_response(),
            AppResponse::Template(template) => match template.render() {
                Ok(html) => (
                    StatusCode::OK,
                    [(CONTENT_TYPE, "text/html; charset=utf-8")],
                    html,
                )
                    .into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(CONTENT_TYPE, "text/plain; charset=utf-8")],
                    format!("Template error: {}", err),
                )
                    .into_response(),
            },
            AppResponse::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                [(CONTENT_TYPE, "text/plain; charset=utf-8")],
                msg,
            )
                .into_response(),
        }
    }
}

// Helper function to strip port from host
fn strip_port(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Read and parse the redirects configuration
    let config_content =
        std::fs::read_to_string("redirects.toml").expect("Failed to read redirects.toml");
    let config: Config = toml::from_str(&config_content).expect("Failed to parse redirects.toml");
    let redirects = Arc::new(config.redirects);

    // Create the router
    let app = Router::new()
        .route("/", get(handle_redirect))
        .route("/*path", get(handle_redirect))
        .layer(TraceLayer::new_for_http())
        .with_state(redirects.clone());

    // Bind to all interfaces
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("listening on {}", addr);

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// Combined handler for both root and path-based requests
async fn handle_redirect(
    Host(host): Host,
    path: Option<Path<String>>,
    redirects: axum::extract::State<Arc<HashMap<String, String>>>,
) -> impl IntoResponse {
    let host = strip_port(&host);
    tracing::debug!("Processing request for host: {}", host);

    // First try subdomain redirect
    if let Some(subdomain) = host.strip_suffix(".rwth.cool") {
        tracing::debug!("Found subdomain: {}", subdomain);
        if let Some(target) = redirects.get(subdomain) {
            tracing::info!("Redirecting {} to {}", host, target);
            return AppResponse::Redirect(Redirect::permanent(target));
        }
    }

    // If no subdomain match, try path-based redirect
    if let Some(Path(path)) = path {
        let path = path.trim_start_matches('/');
        let redirect_key = path.split('/').next().unwrap_or("");
        tracing::debug!("Checking path redirect for: {}", redirect_key);

        if let Some(target) = redirects.get(redirect_key) {
            tracing::info!("Redirecting /{} to {}", redirect_key, target);
            return AppResponse::Redirect(Redirect::permanent(target));
        }
    }

    // If no redirect found and we're on the main domain, show the list
    if host == "rwth.cool" {
        // Convert the HashMap to a static reference - this is safe because redirects lives for the entire program
        let redirects_static = unsafe {
            std::mem::transmute::<&HashMap<String, String>, &'static HashMap<String, String>>(
                &redirects,
            )
        };
        AppResponse::Template(IndexTemplate {
            redirects: redirects_static,
        })
    } else {
        AppResponse::NotFound("Redirect not found".to_string())
    }
}
