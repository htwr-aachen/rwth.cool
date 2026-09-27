//! Validation tests for the shipped `redirects.toml`.

use rwth_cool::build_state;

const BASE_URL: &str = "https://rwth.cool";

#[test]
fn shipped_redirects_are_valid() {
    build_state(include_str!("../redirects.toml"), BASE_URL).expect("redirects.toml must be valid");
}

#[test]
fn unknown_fields_are_rejected() {
    // Regression test: `alias` instead of `aliases` silently disabled aliases.
    let config = r#"
[redirects.foo]
url = "https://example.com"
description = "test"
alias = ["typo"]
"#;
    let err = build_state(config, BASE_URL).unwrap_err();
    assert!(err.contains("alias"), "unexpected error: {err}");
}

#[test]
fn duplicate_aliases_are_rejected() {
    let config = r#"
[redirects.a]
url = "https://example.com/a"
description = "a"
aliases = ["shared"]

[redirects.b]
url = "https://example.com/b"
description = "b"
aliases = ["shared"]
"#;
    let err = build_state(config, BASE_URL).unwrap_err();
    assert!(err.contains("shared"), "unexpected error: {err}");
}

#[test]
fn aliases_colliding_with_shortcuts_are_rejected() {
    let config = r#"
[redirects.a]
url = "https://example.com/a"
description = "a"
aliases = ["b"]

[redirects.b]
url = "https://example.com/b"
description = "b"
"#;
    let err = build_state(config, BASE_URL).unwrap_err();
    assert!(err.contains("collides"), "unexpected error: {err}");
}

#[test]
fn invalid_base_urls_are_rejected() {
    assert!(build_state(include_str!("../redirects.toml"), "rwth.cool").is_err());
}

#[test]
fn base_url_is_normalized() {
    let state = build_state(include_str!("../redirects.toml"), "HTTPS://RWTH.COOL/").unwrap();
    assert_eq!(state.base_url, "https://rwth.cool");
    assert_eq!(state.base_domain, "rwth.cool");
    assert_eq!(state.base_port, "");

    let state = build_state(include_str!("../redirects.toml"), "http://localhost:3210").unwrap();
    assert_eq!(state.base_url, "http://localhost:3210");
    assert_eq!(state.base_domain, "localhost");
    assert_eq!(state.base_port, ":3210");
}
