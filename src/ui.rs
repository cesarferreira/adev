//! Presentation + interaction helpers: device/variant resolution and the
//! human-vs-JSON output split. The `androkit` library stays presentation-free;
//! everything user-facing lives here.

use androkit::adb::Adb;
use androkit::error::{anyhow, Result};
use androkit::model::AndroidProject;
use colored::*;
use inquire::Select;
use std::io::IsTerminal;

/// Resolve which device to target.
///
/// Order: an explicit `--device`, then the sole connected device, then an
/// interactive picker (TTY only). In `--json` mode or without a TTY, ambiguity
/// is an error rather than a prompt.
pub fn select_device(adb: &Adb, pinned: Option<&str>, json: bool) -> Result<String> {
    if let Some(d) = pinned {
        return Ok(d.to_string());
    }
    let devices = adb.devices()?;
    if devices.len() == 1 {
        return Ok(devices[0].clone());
    }
    if json || !std::io::stdin().is_terminal() {
        return Err(anyhow!(
            "Multiple devices connected; pass --device <serial>. Available: {}",
            devices.join(", ")
        ));
    }
    Ok(Select::new("Select a device", devices).prompt()?)
}

/// Resolve which variant to operate on: explicit `--variant`/positional, else
/// the project's resolved default.
pub fn resolve_variant(project: &AndroidProject, explicit: Option<&str>) -> Result<String> {
    if let Some(v) = explicit {
        return Ok(v.to_string());
    }
    project
        .default_variant
        .clone()
        .ok_or_else(|| anyhow!("No build variants found for this project"))
}

/// The application id, or a helpful error when discovery couldn't find one.
pub fn require_application_id(project: &AndroidProject) -> Result<&str> {
    project
        .application_id
        .as_deref()
        .ok_or_else(|| anyhow!("Could not determine applicationId from the project's build files"))
}

/// Print a success line (human mode) — suppressed in JSON mode.
pub fn ok(json: bool, msg: &str) {
    if !json {
        println!("{} {}", "✓".green().bold(), msg);
    }
}

/// Print an informational line (human mode) — suppressed in JSON mode.
pub fn info(json: bool, msg: &str) {
    if !json {
        println!("{msg}");
    }
}

/// Print pretty JSON to stdout.
pub fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use androkit::model::{AndroidProject, Variant};

    /// A project with a single `devDebug` variant and a default + application id.
    fn project() -> AndroidProject {
        AndroidProject {
            root: "/tmp/app".to_string(),
            modules: Vec::new(),
            app_module: Some(":app".to_string()),
            variants: vec![Variant {
                name: "devDebug".to_string(),
                build_type: "debug".to_string(),
                flavors: vec!["dev".to_string()],
            }],
            default_variant: Some("devDebug".to_string()),
            application_id: Some("com.example.app".to_string()),
            launch_activity: Some("com.example.app/.MainActivity".to_string()),
        }
    }

    #[test]
    fn resolve_variant_prefers_explicit_over_default() {
        let p = project();
        assert_eq!(
            resolve_variant(&p, Some("prodRelease")).unwrap(),
            "prodRelease"
        );
    }

    #[test]
    fn resolve_variant_falls_back_to_default() {
        let p = project();
        assert_eq!(resolve_variant(&p, None).unwrap(), "devDebug");
    }

    #[test]
    fn resolve_variant_errors_without_default_or_explicit() {
        let mut p = project();
        p.default_variant = None;
        p.variants.clear();
        assert!(resolve_variant(&p, None).is_err());
    }

    #[test]
    fn resolve_variant_uses_explicit_even_without_default() {
        // An explicit choice should work even when discovery found no default.
        let mut p = project();
        p.default_variant = None;
        assert_eq!(
            resolve_variant(&p, Some("stagingDebug")).unwrap(),
            "stagingDebug"
        );
    }

    #[test]
    fn require_application_id_returns_some() {
        let p = project();
        assert_eq!(require_application_id(&p).unwrap(), "com.example.app");
    }

    #[test]
    fn require_application_id_errors_when_missing() {
        let mut p = project();
        p.application_id = None;
        assert!(require_application_id(&p).is_err());
    }
}
