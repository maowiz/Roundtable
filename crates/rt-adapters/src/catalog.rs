//! Fixed endpoint catalog (SEC-03). Hosts outside this list are never contacted.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CATALOG_JSON: &str = include_str!("../../../contracts/providers/catalog.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: String,
    pub adapter: String,
    pub host: String,
    pub path: String,
    pub auth: String,
    #[serde(default)]
    pub version_header: Option<String>,
}

impl ProviderEntry {
    pub fn url(&self, model: &str) -> String {
        format!("https://{}{}", self.host, self.path.replace("{model}", model))
    }
}

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    pub version: String,
    entries: HashMap<String, ProviderEntry>,
}

impl ProviderCatalog {
    pub fn bundled() -> ProviderCatalog {
        let v: serde_json::Value = serde_json::from_str(CATALOG_JSON).expect("provider catalog parses");
        let version = v["catalog_version"].as_str().unwrap_or("unknown").to_string();
        let entries: Vec<ProviderEntry> = serde_json::from_value(v["providers"].clone()).expect("provider entries parse");
        ProviderCatalog { version, entries: entries.into_iter().map(|e| (e.id.clone(), e)).collect() }
    }

    pub fn get(&self, id: &str) -> Option<&ProviderEntry> {
        self.entries.get(id)
    }

    /// True when `url` points at a listed host over HTTPS.
    pub fn allows(&self, url: &str) -> bool {
        let Some(rest) = url.strip_prefix("https://") else { return false };
        let host = rest.split('/').next().unwrap_or("");
        self.entries.values().any(|e| e.host == host)
    }

    pub fn ids(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.entries.keys().map(String::as_str).collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_has_six_providers_and_enforces_hosts() {
        let c = ProviderCatalog::bundled();
        assert_eq!(c.ids(), vec!["anthropic", "google", "groq", "openai", "openrouter", "xai"]);
        assert!(c.allows("https://api.openai.com/v1/responses"));
        assert!(!c.allows("http://api.openai.com/v1/responses"), "plain http is refused");
        assert!(!c.allows("https://evil.example.com/v1/responses"));
        assert!(!c.allows("https://api.openai.com.evil.example/v1"));
        assert_eq!(c.get("google").unwrap().url("gemini-x"), "https://generativelanguage.googleapis.com/v1beta/models/gemini-x:generateContent");
    }
}
