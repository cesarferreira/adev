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
