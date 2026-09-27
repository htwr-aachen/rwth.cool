//! RWTH.cool: a small link shortener for RWTH Aachen University.

mod config;
mod host;

use askama::Template;
use axum::{
    body::Bytes,
    extract::{OriginalUri, Path, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use std::{collections::HashMap, sync::Arc};
use tower_http::trace::TraceLayer;

use config::{redirects_json, Aliases, Config, RedirectEntry, Redirects};
use host::{extract_subdomain, is_main_domain, normalize_host, port_suffix, strip_www_for};

/// Favicon, embedded into the binary at compile time.
const FAVICON: &[u8] = include_bytes!("../static/favicon.png");
/// Client-side search helper, embedded into the binary at compile time.
const SEARCH_JS: &str = include_str!("../static/search.js");

/// A redirect entry together with its key, used for the index page.
#[derive(Debug)]
struct SortedRedirect<'a> {
    key: &'a str,
    entry: &'a RedirectEntry,
}

/// All redirects of a single category.
#[derive(Debug)]
struct CategoryGroup<'a> {
    category: String,
    redirects: Vec<SortedRedirect<'a>>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    categories: Vec<CategoryGroup<'a>>,
    base_url: &'a str,
    base_domain: &'a str,
}

#[derive(Template)]
#[template(path = "404.html")]
struct NotFoundTemplate<'a> {
    subdomain: &'a str,
    redirects_json: &'a str,
    base_url: &'a str,
    base_domain: &'a str,
}

impl<'a> IndexTemplate<'a> {
    fn new(
        redirects: &'a HashMap<String, RedirectEntry>,
        base_url: &'a str,
        base_domain: &'a str,
    ) -> Self {
        use std::collections::BTreeMap;

        // Group redirects by category; the BTreeMap also sorts the categories.
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

        let categories = category_map
            .into_iter()
            .map(|(category, mut redirects)| {
                redirects.sort_by(|a, b| a.key.cmp(b.key));
                CategoryGroup {
                    category,
                    redirects,
                }
            })
            .collect();

        Self {
            categories,
            base_url,
            base_domain,
        }
    }
}

/// Shared application state, built once at startup.
#[derive(Debug)]
pub struct AppState {
    pub redirects: Redirects,
    pub aliases: Aliases,
    pub index_html: Bytes,
    pub redirects_json: String,
    pub base_url: String,
    pub base_domain: String,
    pub base_port: String,
    pub scheme: String,
}

/// Parses the configuration and prepares all derived data.
pub fn build_state(config_toml: &str, base_url: &str) -> Result<AppState, String> {
    let trimmed = base_url.trim_end_matches('/');
    let parsed: Uri = trimmed
        .parse()
        .map_err(|err| format!("invalid base URL '{base_url}': {err}"))?;
    let scheme = parsed
        .scheme_str()
        .ok_or_else(|| format!("base URL '{base_url}' is missing a scheme (e.g. https://)"))?
        .to_ascii_lowercase();
    let authority = parsed
        .authority()
        .ok_or_else(|| format!("base URL '{base_url}' is missing a host"))?;
    let base_domain = authority.host().to_ascii_lowercase();
    let base_port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    // `Uri` reports the root path as `/`; keep the base URL without a trailing
    // slash so that appending paths never produces a double slash.
    let base_path = match parsed.path() {
        "" | "/" => "",
        path => path,
    };
    let base_url = format!("{scheme}://{base_domain}{base_port}{base_path}");

    let config = Config::parse(config_toml)?;
    let (redirects, aliases) = config.into_maps()?;

    let index_html = IndexTemplate::new(&redirects, &base_url, &base_domain)
        .render()
        .map_err(|err| format!("failed to render index template: {err}"))?;
    let redirects_json = redirects_json(&redirects);

    Ok(AppState {
        redirects,
        aliases,
        index_html: Bytes::from(index_html),
        redirects_json,
        base_url,
        base_domain,
        base_port,
        scheme,
    })
}

/// Builds the router with all routes and layers.
pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/favicon.png", get(favicon))
        .route("/healthz", get(healthz))
        .route("/robots.txt", get(robots_txt))
        .route("/search.js", get(search_js))
        .route("/sitemap.xml", get(sitemap_xml))
        .route("/", get(handle_redirect))
        .route("/{*path}", get(handle_redirect))
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Serves the favicon.
async fn favicon() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        Bytes::from_static(FAVICON),
    )
}

/// Liveness probe for container orchestration.
async fn healthz() -> &'static str {
    "ok"
}

/// Serves `robots.txt`, pointing at the configured base URL.
async fn robots_txt(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = format!(
        "User-agent: *\nAllow: /\n\nSitemap: {}/sitemap.xml\n",
        state.base_url
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body)
}

/// Serves the client-side search helper.
async fn search_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        SEARCH_JS,
    )
}

/// Serves `sitemap.xml` for the configured base URL.
async fn sitemap_xml(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>{}/</loc>
  </url>
</urlset>
"#,
        state.base_url
    );
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
}

/// Main handler: resolves subdomain and path shortcuts, serves the index page
/// on the main domain and the suggestion page otherwise.
#[axum::debug_handler]
async fn handle_redirect(
    State(state): State<Arc<AppState>>,
    OriginalUri(original_uri): OriginalUri,
    path: Option<Path<String>>,
    headers: HeaderMap,
) -> Response {
    let raw_host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let host = normalize_host(raw_host);
    tracing::debug!(host = %host, "processing request");

    // Canonicalize `www.` hosts of our own domain.
    if let Some(stripped) = strip_www_for(&host, &state.base_domain) {
        let path_and_query = original_uri
            .path_and_query()
            .map_or("/", |path_and_query| path_and_query.as_str());
        // Keep the port used by the client, or fall back to the port of BASE_URL
        // (clients may omit the default port).
        let port = match port_suffix(raw_host) {
            "" => state.base_port.as_str(),
            port => port,
        };
        let target = format!("{}://{}{}{}", state.scheme, stripped, port, path_and_query);
        tracing::debug!(from = %host, to = %target, "canonicalizing www host");
        return Redirect::permanent(&target).into_response();
    }

    // Subdomain shortcuts take precedence over path shortcuts.
    if let Some(subdomain) = extract_subdomain(&host) {
        if let Some(target) = find_target(&state, subdomain) {
            tracing::debug!(host = %host, target, "redirecting subdomain");
            return Redirect::temporary(target).into_response();
        }
    }

    if let Some(Path(path)) = &path {
        let key = path.trim_start_matches('/').split('/').next().unwrap_or("");
        if !key.is_empty() {
            if let Some(target) = find_target(&state, key) {
                tracing::debug!(path = %path, target, "redirecting path");
                return Redirect::temporary(target).into_response();
            }
        }
    }

    // On the main domain the index page is served at the root.
    let is_root = path.as_ref().is_none_or(|Path(path)| path.is_empty());
    if is_main_domain(&host) && is_root {
        return (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            state.index_html.clone(),
        )
            .into_response();
    }

    let subdomain = extract_subdomain(&host)
        .map(str::to_string)
        .or_else(|| {
            path.as_ref().and_then(|Path(path)| {
                let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
                (!first.is_empty()).then(|| first.to_string())
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    let template = NotFoundTemplate {
        subdomain: &subdomain,
        redirects_json: &state.redirects_json,
        base_url: &state.base_url,
        base_domain: &state.base_domain,
    };
    match template.render() {
        Ok(html) => (
            StatusCode::NOT_FOUND,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            html,
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("Template error: {err}"),
        )
            .into_response(),
    }
}

/// Looks up a shortcut by key, following aliases.
fn find_target<'a>(state: &'a AppState, key: &str) -> Option<&'a str> {
    if let Some(entry) = state.redirects.get(key) {
        return Some(entry.url.as_str());
    }
    state
        .aliases
        .get(key)
        .and_then(|primary| state.redirects.get(primary))
        .map(|entry| entry.url.as_str())
}

/// Adds a few conservative security headers to every response.
async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), camera=(), microphone=()"),
    );
    response
}
