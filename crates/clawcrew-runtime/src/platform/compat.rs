//! Extension/provider compatibility matrices and local scaffolding (P3.3).
//!
//! Pure version comparison plus a small matrix builder so the same rules gate
//! providers, plugins, Apps, and the gateway. Cross-version behaviour is
//! covered by the unit tests here and by `AppRegistry` activation tests.

use std::collections::HashMap;

use serde::Serialize;
use clawcrew_api::app_manifest::{AppManifest, AppToolDefinition};

/// The running runtime/gateway version this matrix compares against.
pub const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parse a `major.minor.patch` version, ignoring pre-release/build suffixes.
pub fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.split(['-', '+']).next().unwrap_or(value);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether `actual` satisfies a `required_min` floor. Invalid versions are
/// treated as incompatible (fail closed).
pub fn is_compatible(required_min: &str, actual: &str) -> bool {
    match (parse_version(required_min), parse_version(actual)) {
        (Some(min), Some(actual)) => actual >= min,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompatEntry {
    /// Stable identifier (App/plugin/provider id).
    pub name: String,
    /// One of `provider` / `plugin` / `app` / `gateway`.
    pub kind: String,
    /// The minimum version the extension requires.
    pub required_min: String,
    /// The version actually available.
    pub actual: String,
    pub compatible: bool,
    /// Human-readable reason when incompatible.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CompatMatrix {
    entries: Vec<CompatEntry>,
}

impl CompatMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, kind: &str, name: &str, required_min: &str, actual: &str) {
        let compatible = is_compatible(required_min, actual);
        self.entries.push(CompatEntry {
            name: name.to_string(),
            kind: kind.to_string(),
            required_min: required_min.to_string(),
            actual: actual.to_string(),
            compatible,
            reason: (!compatible).then(|| {
                format!("requires >= {required_min}, available {actual}")
            }),
        });
    }

    /// Check an App's `min_runtime_version` against the gateway version.
    pub fn check_app(&mut self, id: &str, min_runtime_version: &str, gateway_version: &str) {
        self.push("app", id, min_runtime_version, gateway_version);
    }

    /// Check a gateway/peer's required minimum against the available version.
    pub fn check_gateway(&mut self, name: &str, required_min: &str, actual: &str) {
        self.push("gateway", name, required_min, actual);
    }

    pub fn entries(&self) -> &[CompatEntry] {
        &self.entries
    }

    pub fn all_compatible(&self) -> bool {
        self.entries.iter().all(|entry| entry.compatible)
    }

    pub fn incompatible(&self) -> Vec<&CompatEntry> {
        self.entries.iter().filter(|entry| !entry.compatible).collect()
    }
}

/// Render the matrix as Markdown for the capability docs.
pub fn render_compat_markdown(matrix: &CompatMatrix) -> String {
    let mut out = String::from("# Compatibility Matrix\n\n");
    out.push_str(&format!("Runtime version: `{RUNTIME_VERSION}`\n\n"));
    out.push_str("| Kind | Name | Requires | Available | Status |\n|---|---|---|---|---|\n");
    for entry in matrix.entries() {
        out.push_str(&format!(
            "| {} | `{}` | {} | {} | {} |\n",
            entry.kind,
            entry.name,
            entry.required_min,
            entry.actual,
            if entry.compatible {
                "ok"
            } else {
                entry.reason.as_deref().unwrap_or("incompatible")
            },
        ));
    }
    out
}

/// Build a local App scaffold manifest with a placeholder tool and lifecycle
/// hook. The result passes `AppRegistry` manifest validation as-is.
pub fn scaffold_app_manifest(id: &str, name: &str, version: &str) -> AppManifest {
    let mut ui_routes = HashMap::new();
    ui_routes.insert("home".to_string(), "/apps/{id}".replace("{id}", id));
    AppManifest {
        mcp_server: None,
        id: id.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        min_runtime_version: RUNTIME_VERSION.to_string(),
        dependencies: Vec::new(),
        permissions: Vec::new(),
        tools: vec![AppToolDefinition {
            name: format!("{id}_tool"),
            description: format!("Placeholder tool for {name}"),
            entrypoint: "main".to_string(),
        }],
        config: serde_json::json!({}),
        ui_routes,
        lifecycle_hooks: vec!["on_enable".to_string(), "on_disable".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::app_registry::AppRegistry;

    #[test]
    fn version_parsing_ignores_suffixes_and_rejects_junk() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3-beta.1+build"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("not-a-version"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn compatibility_ordering_is_correct_and_fails_closed() {
        assert!(is_compatible("1.0.0", "1.0.0"));
        assert!(is_compatible("1.0.0", "1.2.0"));
        assert!(!is_compatible("2.0.0", "1.9.9"));
        assert!(!is_compatible("garbage", "1.0.0"));
        assert!(!is_compatible("1.0.0", "garbage"));
    }

    #[test]
    fn matrix_flags_incompatible_entries() {
        let mut matrix = CompatMatrix::new();
        matrix.check_app("good", "0.8.0", "0.8.5");
        matrix.check_app("bad", "9.0.0", "0.8.5");
        matrix.check_gateway("peer", "1.0.0", "0.9.0");
        assert!(!matrix.all_compatible());
        assert_eq!(matrix.incompatible().len(), 2);
        let markdown = render_compat_markdown(&matrix);
        assert!(markdown.contains("| app | `good` |"));
        assert!(markdown.contains("9.0.0"));
    }

    #[test]
    fn scaffold_manifest_registers_and_activates() {
        let manifest = scaffold_app_manifest("demo", "Demo", "0.1.0");
        let mut registry = AppRegistry::new();
        registry.register_app(manifest).unwrap();
        registry.enable_app("demo", RUNTIME_VERSION).unwrap();
        assert!(registry.get_app("demo").is_some());
    }
}