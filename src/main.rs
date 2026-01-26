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

static REDIRECTS_MAP: OnceCell<HashMap<String, RedirectEntry>> = OnceCell::new();

/// Check if a host is considered a "main domain" host (where we show the index page)
/// Returns true for hosts without subdomains (localhost, IPs, or two-part domains like example.com)
fn is_main_domain(host: &str) -> bool {
    // No dots means no subdomain (localhost, single-label hostname)
    if !host.contains('.') {
        return true;
    }
    
    // IP addresses are main domains
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    
    // Count the number of parts (dots + 1)
    // Two parts (e.g., "rwth.cool", "example.com") = main domain
    // Three+ parts (e.g., "moodle.rwth.cool") = has subdomain
    let parts = host.split('.').count();
    parts <= 2
}

/// Extract subdomain from host (first part before the first dot)
/// Only returns a subdomain if the host has 3+ parts (subdomain.domain.tld)
fn extract_subdomain(host: &str) -> Option<&str> {
    // Don't treat IP address octets as subdomains
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return None;
    }
    
    // Need at least 3 parts for a subdomain (subdomain.domain.tld)
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    
    // First part is the subdomain
    Some(parts[0])
}

#[derive(Debug, Deserialize, Clone)]
struct RedirectEntry {
    url: String,
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    category: Option<String>,
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

// Category group for organizing redirects
#[derive(Debug)]
struct CategoryGroup<'a> {
    category: String,
    redirects: Vec<SortedRedirect<'a>>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    categories: Vec<CategoryGroup<'a>>,
}

impl<'a> IndexTemplate<'a> {
    fn new(redirects: &'a HashMap<String, RedirectEntry>) -> Self {
        use std::collections::BTreeMap;

        // Group redirects by category
        let mut category_map: BTreeMap<String, Vec<SortedRedirect>> = BTreeMap::new();

        for (key, entry) in redirects {
            let category = entry
                .category
                .clone()
                .unwrap_or_else(|| "Other".to_string());
            category_map
                .entry(category)
                .or_default()
                .push(SortedRedirect { key, entry });
        }

        // Sort redirects within each category and convert to CategoryGroup
        let categories: Vec<CategoryGroup> = category_map
            .into_iter()
            .map(|(category, mut redirects)| {
                redirects.sort_by(|a, b| a.key.cmp(b.key));
                CategoryGroup {
                    category,
                    redirects,
                }
            })
            .collect();

        // Categories are already sorted by BTreeMap

        Self { categories }
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
    if let Some(subdomain) = extract_subdomain(host) {
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
    if is_main_domain(host) && {
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
