use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    routing::get,
    Router,
};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DOMAIN: &str = "rwth.cool";
static REDIRECTS_MAP: OnceCell<HashMap<String, RedirectEntry>> = OnceCell::new();

#[derive(Debug, Deserialize, Clone)]
struct RedirectEntry {
    url: String,
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Config {
    redirects: HashMap<String, RedirectEntry>,
}

// New type to hold a sorted redirect entry
#[derive(Debug)]
struct SortedRedirect<'a> {
    key: &'a str,
    entry: &'a RedirectEntry,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    redirects: Vec<SortedRedirect<'a>>,
}

impl<'a> IndexTemplate<'a> {
    fn new(redirects: &'a HashMap<String, RedirectEntry>) -> Self {
        let mut sorted_redirects: Vec<SortedRedirect> = redirects
            .iter()
            .map(|(key, entry)| SortedRedirect { key, entry })
            .collect();

        sorted_redirects.sort_by(|a, b| a.key.cmp(b.key));

        Self {
            redirects: sorted_redirects,
        }
    }
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
                    format!("Template error: {err}"),
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

// Favicon handler
async fn favicon() -> impl IntoResponse {
    match std::fs::read("static/favicon.png") {
        Ok(content) => (StatusCode::OK, [(CONTENT_TYPE, "image/x-icon")], content).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(CONTENT_TYPE, "text/plain")],
            "Favicon not found".to_string(),
        )
            .into_response(),
    }
}

// Type aliases to simplify complex types
type RedirectMap = Arc<HashMap<String, RedirectEntry>>;
type AliasMap = Arc<HashMap<String, String>>;
type AppState = (RedirectMap, AliasMap);

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

    // Initialize the static redirects map
    REDIRECTS_MAP
        .set(config.redirects.clone())
        .expect("Failed to initialize static redirects map");

    // Create a map of aliases to their primary keys
    let mut aliases_map = HashMap::new();
    for (key, entry) in REDIRECTS_MAP.get().unwrap() {
        for alias in &entry.aliases {
            aliases_map.insert(alias.clone(), key.clone());
        }
    }

    let redirects = Arc::new(REDIRECTS_MAP.get().unwrap().clone());
    let aliases_map = Arc::new(aliases_map);

    // Create the router
    let app = Router::new()
        .route("/favicon.png", get(favicon))
        .route("/", get(handle_redirect))
        .route("/{*path}", get(handle_redirect))
        .layer(TraceLayer::new_for_http())
        .with_state((redirects.clone(), aliases_map.clone()));

    // Bind to all interfaces
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("listening on {}", addr);

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// Combined handler for both root and path-based requests
#[axum::debug_handler]
async fn handle_redirect(
    State((redirects, aliases_map)): State<AppState>,
    path: Option<Path<String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let host = strip_port(host);
    tracing::debug!("Processing request for host: {}", host);

    // First try subdomain redirect
    if let Some(subdomain) = host.strip_suffix(&format!(".{DOMAIN}")) {
        tracing::debug!("Found subdomain: {}", subdomain);

        // Check direct redirects first
        if let Some(target) = redirects.get(subdomain) {
            tracing::info!("Redirecting {} to {}", host, target.url);
            return AppResponse::Redirect(Redirect::permanent(&target.url));
        }

        // Then check aliases
        if let Some(primary_key) = aliases_map.get(subdomain) {
            if let Some(target) = redirects.get(primary_key) {
                tracing::info!("Redirecting {} (alias) to {}", host, target.url);
                return AppResponse::Redirect(Redirect::permanent(&target.url));
            }
        }
    }

    // If no subdomain match, try path-based redirect
    if let Some(Path(path)) = &path {
        let path = path.trim_start_matches('/');
        let redirect_key = path.split('/').next().unwrap_or("");
        tracing::debug!("Checking path redirect for: {}", redirect_key);

        // Check direct redirects first
        if let Some(target) = redirects.get(redirect_key) {
            tracing::info!("Redirecting /{} to {}", redirect_key, target.url);
            return AppResponse::Redirect(Redirect::permanent(&target.url));
        }

        // Then check aliases
        if let Some(primary_key) = aliases_map.get(redirect_key) {
            if let Some(target) = redirects.get(primary_key) {
                tracing::info!("Redirecting /{} (alias) to {}", redirect_key, target.url);
                return AppResponse::Redirect(Redirect::permanent(&target.url));
            }
        }
    }

    // If no redirect found and we're on the main domain, show the list
    if host == DOMAIN && {
        if let Some(Path(path)) = path {
            path.is_empty()
        } else {
            true
        }
    } {
        AppResponse::Template(IndexTemplate::new(REDIRECTS_MAP.get().unwrap()))
    } else {
        AppResponse::NotFound("# Redirect not found".to_string())
    }
}
