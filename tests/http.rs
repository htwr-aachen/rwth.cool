//! End-to-end tests for the HTTP routes.

use axum::{
    body::Body,
    http::{header, HeaderMap, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

const BASE_URL: &str = "https://rwth.cool";

fn test_app() -> Router {
    let state = rwth_cool::build_state(include_str!("../redirects.toml"), BASE_URL)
        .expect("test configuration must be valid");
    rwth_cool::app(Arc::new(state))
}

async fn get(host: &str, uri: &str) -> (StatusCode, HeaderMap, String) {
    let response = test_app()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::HOST, host)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn subdomain_shortcut_redirects() {
    let (status, headers, _) = get("mensa.rwth.cool", "/").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert!(headers[header::LOCATION]
        .to_str()
        .unwrap()
        .contains("studierendenwerk-aachen.de"));
}

#[tokio::test]
async fn path_shortcut_redirects() {
    let (status, headers, _) = get("rwth.cool", "/quit").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert!(headers[header::LOCATION]
        .to_str()
        .unwrap()
        .contains("paperless"));
}

#[tokio::test]
async fn path_shortcut_ignores_trailing_segments() {
    let (status, _, _) = get("rwth.cool", "/mensa/123").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
}

#[tokio::test]
async fn aliases_redirect() {
    for host in ["getmeoutofrwth.rwth.cool", "aufgeben.rwth.cool"] {
        let (status, headers, _) = get(host, "/").await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "host: {host}");
        assert!(headers[header::LOCATION]
            .to_str()
            .unwrap()
            .contains("paperless"));
    }
}

#[tokio::test]
async fn alias_path_redirects() {
    let (status, _, _) = get("rwth.cool", "/getmeoutofrwth").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
}

#[tokio::test]
async fn hosts_are_case_insensitive() {
    let (status, _, _) = get("QUIT.RWTH.COOL", "/").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
}

#[tokio::test]
async fn www_host_is_canonicalized() {
    let (status, headers, _) = get("www.rwth.cool", "/?q=1").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(headers[header::LOCATION], "https://rwth.cool/?q=1");
}

#[tokio::test]
async fn www_subdomain_is_canonicalized() {
    let (status, headers, _) = get("www.mensa.rwth.cool", "/").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(headers[header::LOCATION], "https://mensa.rwth.cool/");
}

#[tokio::test]
async fn repeated_www_prefix_is_stripped_in_one_hop() {
    let (status, headers, _) = get("www.www.rwth.cool", "/").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(headers[header::LOCATION], "https://rwth.cool/");
}

#[tokio::test]
async fn shortcuts_are_case_insensitive_in_the_config() {
    let config = r#"
[redirects.Mensa]
url = "https://example.com/mensa"
description = "Mensa"
aliases = ["HUNGER"]
"#;
    let state = rwth_cool::build_state(config, BASE_URL).unwrap();
    let app = rwth_cool::app(Arc::new(state));

    for host in ["mensa.rwth.cool", "hunger.rwth.cool"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::HOST, host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::TEMPORARY_REDIRECT,
            "host: {host}"
        );
    }
}

#[tokio::test]
async fn index_page_exposes_the_base_domain_to_search() {
    let (status, _, body) = get("rwth.cool", "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"data-base-domain="rwth.cool""#));
}

#[tokio::test]
async fn index_page_is_served_on_the_main_domain() {
    let (status, _, body) = get("rwth.cool", "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Quick access to RWTH Aachen University resources"));
}

#[tokio::test]
async fn unknown_shortcut_returns_not_found() {
    let (status, headers, body) = get("nope.rwth.cool", "/").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(body.contains("nope"));
}

#[tokio::test]
async fn not_found_page_contains_valid_json() {
    let (status, _, body) = get("nope.rwth.cool", "/").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let json = body
        .split_once(r#"<script id="redirects-data" type="application/json">"#)
        .expect("redirect JSON block")
        .1
        .split_once("</script>")
        .expect("end of redirect JSON block")
        .0;
    let parsed: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
    assert!(parsed.get("mensa").is_some());
}

#[tokio::test]
async fn favicon_is_served_as_png() {
    let (status, headers, _) = get("rwth.cool", "/favicon.png").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
}

#[tokio::test]
async fn health_endpoint_responds() {
    let (status, _, body) = get("rwth.cool", "/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn robots_points_at_the_configured_base_url() {
    let (status, _, body) = get("rwth.cool", "/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sitemap: https://rwth.cool/sitemap.xml"));
}

#[tokio::test]
async fn sitemap_points_at_the_configured_base_url() {
    let (status, _, body) = get("rwth.cool", "/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<loc>https://rwth.cool/</loc>"));
}

#[tokio::test]
async fn security_headers_are_present() {
    let (_, headers, _) = get("rwth.cool", "/").await;
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
    assert!(headers.contains_key(header::STRICT_TRANSPORT_SECURITY));
    assert!(headers.contains_key("permissions-policy"));
}
