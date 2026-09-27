//! Configuration file parsing and validation.

use http::Uri;
use serde::Deserialize;
use std::collections::HashMap;

/// A single shortcut, as defined in `redirects.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedirectEntry {
    pub url: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
}

/// The top-level structure of `redirects.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub redirects: HashMap<String, RedirectEntry>,
}

/// Shortcuts by their key.
pub type Redirects = HashMap<String, RedirectEntry>;
/// Aliases mapped to their shortcut key.
pub type Aliases = HashMap<String, String>;

impl Config {
    /// Parses the TOML configuration.
    pub fn parse(toml_str: &str) -> Result<Self, String> {
        toml::from_str(toml_str).map_err(|err| format!("failed to parse redirects: {err}"))
    }

    /// Builds the redirect and alias lookup maps, rejecting invalid or
    /// ambiguous configurations.
    pub fn into_maps(self) -> Result<(Redirects, Aliases), String> {
        // Hosts are matched case-insensitively, so keys and aliases are stored
        // lowercased; anything else would never be reachable.
        let mut redirects: Redirects = HashMap::with_capacity(self.redirects.len());
        for (key, mut entry) in self.redirects {
            let key = key.to_lowercase();
            entry.aliases = entry
                .aliases
                .iter()
                .map(|alias| alias.to_lowercase())
                .collect();
            if redirects.insert(key.clone(), entry).is_some() {
                return Err(format!(
                    "shortcut '{key}' is defined more than once (shortcuts are case-insensitive)"
                ));
            }
        }

        // Iterate in a deterministic order so that error messages are stable.
        let mut keys: Vec<&String> = redirects.keys().collect();
        keys.sort();

        let mut aliases = HashMap::new();
        for key in keys {
            let entry = &redirects[key];
            let url: Uri = entry
                .url
                .parse()
                .map_err(|err| format!("redirect '{key}': invalid url '{}': {err}", entry.url))?;
            if !matches!(url.scheme_str(), Some("http" | "https")) || url.authority().is_none() {
                return Err(format!(
                    "redirect '{key}': url '{}' must be an absolute http:// or https:// URL",
                    entry.url
                ));
            }
            if entry.description.trim().is_empty() {
                return Err(format!("redirect '{key}': description must not be empty"));
            }
            for alias in &entry.aliases {
                if alias.is_empty() {
                    return Err(format!("redirect '{key}': aliases must not be empty"));
                }
                if redirects.contains_key(alias) {
                    return Err(format!(
                        "redirect '{key}': alias '{alias}' collides with an existing shortcut"
                    ));
                }
                if let Some(previous) = aliases.insert(alias.clone(), key.clone()) {
                    return Err(format!(
                        "alias '{alias}' is defined by both '{previous}' and '{key}'"
                    ));
                }
            }
        }

        Ok((redirects, aliases))
    }
}

/// Serializes the redirects into a JSON string that is safe to embed inside a
/// `<script>` element (`<`, `>` and `&` are escaped as unicode sequences).
pub fn redirects_json(redirects: &HashMap<String, RedirectEntry>) -> String {
    let mut map = serde_json::Map::new();
    for (key, entry) in redirects {
        map.insert(
            key.clone(),
            serde_json::json!({
                "url": entry.url,
                "description": entry.description,
                "aliases": entry.aliases,
            }),
        );
    }
    serde_json::Value::Object(map)
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(entries: &str) -> String {
        format!("[redirects]\n{entries}")
    }

    #[test]
    fn rejects_unknown_fields() {
        let result = Config::parse(&config(
            "[redirects.foo]\nurl = \"https://example.com\"\ndescription = \"foo\"\nalias = [\"typo\"]\n",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_duplicate_aliases() {
        let result = Config::parse(&config(
            "[redirects.a]\nurl = \"https://example.com/a\"\ndescription = \"a\"\naliases = [\"shared\"]\n\n[redirects.b]\nurl = \"https://example.com/b\"\ndescription = \"b\"\naliases = [\"shared\"]\n",
        ))
        .unwrap()
        .into_maps();
        let err = result.unwrap_err();
        assert!(err.contains("shared"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_alias_colliding_with_shortcut() {
        let result = Config::parse(&config(
            "[redirects.a]\nurl = \"https://example.com/a\"\ndescription = \"a\"\naliases = [\"b\"]\n\n[redirects.b]\nurl = \"https://example.com/b\"\ndescription = \"b\"\n",
        ))
        .unwrap()
        .into_maps();
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_urls() {
        let result = Config::parse(&config(
            "[redirects.a]\nurl = \"example.com\"\ndescription = \"a\"\n",
        ))
        .unwrap()
        .into_maps();
        assert!(result.is_err());
    }

    #[test]
    fn rejects_incomplete_urls() {
        for url in ["https://", "http://", "example.com", "ftp://example.com"] {
            let toml =
                format!("[redirects]\n[redirects.a]\nurl = \"{url}\"\ndescription = \"a\"\n");
            let result = Config::parse(&toml).unwrap().into_maps();
            assert!(result.is_err(), "url '{url}' should be rejected");
        }
    }

    #[test]
    fn normalizes_keys_and_aliases_to_lowercase() {
        let config = Config::parse(&config(
            "[redirects.Mensa]\nurl = \"https://example.com\"\ndescription = \"mensa\"\naliases = [\"HUNGER\"]\n",
        ))
        .unwrap();
        let (redirects, aliases) = config.into_maps().unwrap();
        assert!(redirects.contains_key("mensa"));
        assert_eq!(redirects["mensa"].aliases, vec!["hunger".to_string()]);
        assert_eq!(aliases.get("hunger").map(String::as_str), Some("mensa"));
    }

    #[test]
    fn rejects_shortcuts_differing_only_by_case() {
        let result = Config::parse(&config(
            "[redirects.mensa]\nurl = \"https://example.com/a\"\ndescription = \"a\"\n\n[redirects.Mensa]\nurl = \"https://example.com/b\"\ndescription = \"b\"\n",
        ))
        .unwrap()
        .into_maps();
        assert!(result.is_err());
    }

    #[test]
    fn escapes_script_unsafe_characters_in_json() {
        let config = Config::parse(&config(
            "[redirects.a]\nurl = \"https://example.com/?a=1&b=2\"\ndescription = \"</script><b>\"\n",
        ))
        .unwrap();
        let (redirects, _) = config.into_maps().unwrap();
        let json = redirects_json(&redirects);
        assert!(!json.contains('<'));
        assert!(!json.contains('>'));
        assert!(!json.contains('&'));
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
    }
}
