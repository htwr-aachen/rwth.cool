//! Helpers for parsing and normalizing `Host` headers.

use std::net::IpAddr;

/// Normalizes a `Host` header value: trims whitespace, strips the port,
/// removes a trailing dot and lowercases the result.
pub fn normalize_host(host: &str) -> String {
    let host = strip_port(host.trim());
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Strips the port from a host, handling IPv6 literals like `[::1]:3000`.
pub fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.find(']').map_or(host, |end| &rest[..end]);
    }
    host.split(':').next().unwrap_or(host)
}

/// Returns the `:port` suffix of a host, or an empty string if there is none.
pub fn port_suffix(host: &str) -> &str {
    let start = host.find(']').map_or(0, |end| end + 1);
    match host[start..].find(':') {
        Some(offset) => &host[start + offset..],
        None => "",
    }
}

/// Returns `true` for hosts that serve the index page: hosts without a
/// subdomain (localhost, IP addresses, or two-part domains like `example.com`).
pub fn is_main_domain(host: &str) -> bool {
    if host.is_empty() || !host.contains('.') {
        return true;
    }
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }

    let mut parts = host.split('.');
    let first = parts.next();
    let second = parts.next();
    let third = parts.next();

    // `mensa.localhost` is treated as a subdomain for local development.
    if third.is_none() && second == Some("localhost") && first != Some("localhost") {
        return false;
    }

    third.is_none()
}

/// Extracts the subdomain from a host, but only when there are at least three
/// parts (`subdomain.domain.tld`). IP addresses are never treated as subdomains.
pub fn extract_subdomain(host: &str) -> Option<&str> {
    if host.is_empty() || host.parse::<IpAddr>().is_ok() {
        return None;
    }

    let mut parts = host.split('.');
    let subdomain = parts.next()?;
    match parts.next()? {
        // `mensa.localhost` is a subdomain for local development.
        "localhost" => Some(subdomain),
        _ => {
            parts.next()?;
            Some(subdomain)
        }
    }
}

/// Returns the host without its leading `www.` when the host belongs to
/// `base_domain`, so that `www.rwth.cool` can be redirected to `rwth.cool`.
pub fn strip_www_for<'a>(host: &'a str, base_domain: &str) -> Option<&'a str> {
    let mut stripped = host;
    while let Some(rest) = stripped.strip_prefix("www.") {
        stripped = rest;
    }
    if stripped == host {
        return None;
    }

    let belongs_to_base = stripped == base_domain
        || stripped
            .strip_suffix(base_domain)
            .is_some_and(|prefix| prefix.ends_with('.'));
    belongs_to_base.then_some(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_ports_and_trailing_dots() {
        assert_eq!(normalize_host("QUIT.RWTH.COOL:3000"), "quit.rwth.cool");
        assert_eq!(normalize_host(" mensa.rwth.cool. "), "mensa.rwth.cool");
        assert_eq!(normalize_host("[::1]:3000"), "::1");
    }

    #[test]
    fn detects_main_domains() {
        assert!(is_main_domain(""));
        assert!(is_main_domain("localhost"));
        assert!(is_main_domain("rwth.cool"));
        assert!(is_main_domain("127.0.0.1"));
        assert!(is_main_domain("::1"));
        assert!(!is_main_domain("mensa.rwth.cool"));
        assert!(!is_main_domain("mensa.localhost"));
    }

    #[test]
    fn extracts_subdomains() {
        assert_eq!(extract_subdomain("mensa.rwth.cool"), Some("mensa"));
        assert_eq!(extract_subdomain("a.b.c.rwth.cool"), Some("a"));
        assert_eq!(extract_subdomain("mensa.localhost"), Some("mensa"));
        assert_eq!(extract_subdomain("rwth.cool"), None);
        assert_eq!(extract_subdomain("localhost"), None);
        assert_eq!(extract_subdomain("127.0.0.1"), None);
        assert_eq!(extract_subdomain("::1"), None);
    }

    #[test]
    fn detects_www_hosts_of_the_base_domain() {
        assert_eq!(
            strip_www_for("www.rwth.cool", "rwth.cool"),
            Some("rwth.cool")
        );
        assert_eq!(
            strip_www_for("www.mensa.rwth.cool", "rwth.cool"),
            Some("mensa.rwth.cool")
        );
        assert_eq!(
            strip_www_for("www.localhost", "localhost"),
            Some("localhost")
        );
        assert_eq!(
            strip_www_for("www.www.rwth.cool", "rwth.cool"),
            Some("rwth.cool")
        );
        assert_eq!(strip_www_for("rwth.cool", "rwth.cool"), None);
        assert_eq!(strip_www_for("www.evil.com", "rwth.cool"), None);
        assert_eq!(strip_www_for("wwwevil.com", "rwth.cool"), None);
    }

    #[test]
    fn extracts_port_suffixes() {
        assert_eq!(port_suffix("rwth.cool"), "");
        assert_eq!(port_suffix("rwth.cool:3000"), ":3000");
        assert_eq!(port_suffix("[::1]:3000"), ":3000");
        assert_eq!(port_suffix("[::1]"), "");
    }
}
