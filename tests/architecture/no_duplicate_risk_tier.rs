//! Architecture gate: risk-tier & fleet-policy facts are the Go engine's single
//! source of truth (`engine/src/fleet/services.go` → `GET /api/fleet/policies`).
//! The web tier (`web-2/`) must CONSUME them over HTTP, never re-declare them.
//!
//! This complements `no_duplicate_state.rs` (which guards peer-authorization
//! fields inside the Rust channel crates) by guarding the Rust↔Go↔TS domain
//! fact the SSOT migration (docs/finalize/08, F4-2) moved into Go.

use std::fs;
use std::path::{Path, PathBuf};

/// TS identifiers that re-declare the risk-tier / policy tables. Their facts
/// (tier, approvalRequired, maxBudgetUSD, enforcement) live in Go. A TS copy is
/// duplicate domain state. Add `// SOT: <reason>` on the line to override.
const FORBIDDEN_TS_SYMBOLS: &[&str] = &["initialRiskTiers", "initialFleetPolicies"];

/// Web tier roots to scan for the forbidden re-declarations.
const SCAN_ROOTS: &[&str] = &["web-2/src"];

#[test]
fn web_tier_does_not_redeclare_risk_tier_or_policy_state() {
    let workspace_root = workspace_root();
    let mut violations: Vec<String> = Vec::new();
    for root in SCAN_ROOTS {
        scan_dir(&workspace_root.join(root), &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Duplicate risk-tier/policy state detected in the web tier. These facts \
         are the Go engine's SSOT (GET /api/fleet/policies, engine/src/fleet/services.go); \
         web-2 must fetch them, not define them — see .kiro/steering/galleon-product-fundamentals.md \
         (risk-tier SSOT) and docs/finalize/08 (F4-2). \
         To override, add `// SOT: <reason>` on the offending line.\n\n\
         Violations:\n{}",
        violations.join("\n")
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scan_dir(dir: &Path, violations: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, violations);
            continue;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some("ts") | Some("tsx") => {}
            _ => continue,
        }
        let Ok(src) = fs::read_to_string(&path) else {
            continue;
        };
        let display = path.display().to_string();
        for (lineno, line) in src.lines().enumerate() {
            if line.contains("// SOT:") {
                continue;
            }
            // Only a DECLARATION is a duplicate; a mention in a comment or an
            // import is not. Match `export const <sym>` / `const <sym>` / `<sym> =`.
            for sym in FORBIDDEN_TS_SYMBOLS {
                let decl_export = format!("export const {sym}");
                let decl_const = format!("const {sym}");
                let decl_assign = format!("{sym} =");
                let is_comment = line.trim_start().starts_with("//");
                if is_comment {
                    continue;
                }
                if line.contains(&decl_export)
                    || line.contains(&decl_const)
                    || (line.contains(&decl_assign) && !line.contains("import"))
                {
                    violations.push(format!("  {}:{}: {}", display, lineno + 1, line.trim_start()));
                }
            }
        }
    }
}
