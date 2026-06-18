//! Command implementations. Each maps a CLI command to `androkit` calls, then
//! renders the result (human text or JSON) via [`crate::ui`].

use crate::cli::Command;
use crate::ui;
use androkit::adb::Adb;
use androkit::error::{anyhow, bail, Context, Result};
use androkit::gradle::Gradle;
use androkit::model::AndroidProject;
use androkit::project;
use colored::*;
use inquire::Confirm;
use serde_json::json;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Shared flags threaded through every command.
pub struct Ctx {
    pub json: bool,
    pub device: Option<String>,
    pub variant: Option<String>,
    pub module: Option<String>,
}

impl Ctx {
    /// Discover the project rooted at the current directory.
    fn project(&self) -> Result<AndroidProject> {
        project::discover(&std::env::current_dir()?)
    }

    /// A Gradle handle for the discovered project root.
    fn gradle(&self, project: &AndroidProject) -> Result<Gradle> {
        Gradle::at(Path::new(&project.root))
    }

    /// Prefix a task with `--module` when set: `installDevDebug` → `:app:installDevDebug`.
    fn scoped(&self, task: &str) -> String {
        match &self.module {
            Some(m) => format!("{}:{}", m.trim_end_matches(':'), task),
            None => task.to_string(),
        }
    }
}

/// Entry point used by `main`.
pub fn run(ctx: &Ctx, command: Command) -> Result<()> {
    match command {
        Command::Info => info(ctx),
        Command::Install { variant } => install(ctx, variant),
        Command::Launch => launch(ctx),
        Command::Test { fresh } => test(ctx, fresh),
        Command::Logs => logs(ctx),
        Command::Clean => clean(ctx),
        Command::DeepClean { yes } => deep_clean(ctx, yes),
        Command::Stop => stop(ctx),
        Command::ClearData => clear_data(ctx),
        Command::Restart => restart(ctx),
        Command::Devices => devices(ctx),
        Command::Screenshot { output } => screenshot(ctx, output),
        Command::Record { output } => record(ctx, output),
    }
}

fn info(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    if ctx.json {
        ui::print_json(&serde_json::to_value(&project)?);
        return Ok(());
    }

    println!("{}", "Project".bold().underline().yellow());
    if let Some(app_id) = &project.application_id {
        println!("{:<16}: {}", "App ID".cyan(), app_id.green());
    }
    if let Some(activity) = &project.launch_activity {
        println!("{:<16}: {}", "Launch".cyan(), activity.green());
    }
    println!("{:<16}: {}", "Root".cyan(), project.root.dimmed());

    println!("\n{}", "Modules".bold().underline().yellow());
    for m in &project.modules {
        let tag = if m.is_application {
            " (app)".green()
        } else {
            "".normal()
        };
        println!("  {}{}", m.path.cyan(), tag);
    }

    println!("\n{}", "Variants".bold().underline().yellow());
    for v in &project.variants {
        let marker = if Some(&v.name) == project.default_variant.as_ref() {
            "→".green().bold()
        } else {
            " ".normal()
        };
        println!("  {} {}", marker, v.name);
    }

    if let Some(default) = &project.default_variant {
        println!(
            "\n{}",
            "Resolved tasks (default variant)"
                .bold()
                .underline()
                .yellow()
        );
        println!(
            "  {:<10}: {}",
            "install".cyan(),
            project.install_task(default)
        );
        println!(
            "  {:<10}: {}",
            "test".cyan(),
            project.unit_test_task(default)
        );
        println!(
            "  {:<10}: {}",
            "assemble".cyan(),
            project.assemble_task(default)
        );
    }
    Ok(())
}

fn install(ctx: &Ctx, positional_variant: Option<String>) -> Result<()> {
    let project = ctx.project()?;
    let variant = ui::resolve_variant(
        &project,
        positional_variant.as_deref().or(ctx.variant.as_deref()),
    )?;
    let task = ctx.scoped(&project.install_task(&variant));
    ui::info(
        ctx.json,
        &format!("Installing {} ({})…", variant.bold(), task.dimmed()),
    );
    run_gradle(ctx, &project, &task, &[])?;
    ui::ok(ctx.json, &format!("Installed {variant}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "variant": variant, "task": task }));
    }
    Ok(())
}

fn launch(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    let component = project
        .launch_activity
        .clone()
        .ok_or_else(|| anyhow!("Could not determine the launcher activity from the manifest"))?;
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    adb.start_activity(&device, &component)?;
    ui::ok(ctx.json, &format!("Launched {component}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "component": component, "device": device }));
    }
    Ok(())
}

fn test(ctx: &Ctx, fresh: bool) -> Result<()> {
    let project = ctx.project()?;
    let variant = ui::resolve_variant(&project, ctx.variant.as_deref())?;
    let task = ctx.scoped(&project.unit_test_task(&variant));
    let extra: &[&str] = if fresh {
        &["--rerun-tasks", "--no-build-cache"]
    } else {
        &[]
    };
    ui::info(
        ctx.json,
        &format!("Testing {} ({})…", variant.bold(), task.dimmed()),
    );
    run_gradle(ctx, &project, &task, extra)?;
    ui::ok(ctx.json, &format!("Tests passed: {variant}"));
    if ctx.json {
        ui::print_json(
            &json!({ "success": true, "variant": variant, "task": task, "fresh": fresh }),
        );
    }
    Ok(())
}

fn logs(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?;
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    let pid = adb.pid_of(&device, app_id)?;
    match &pid {
        Some(pid) => ui::info(
            ctx.json,
            &format!("Streaming logs for {app_id} (pid {pid}). Ctrl+C to stop."),
        ),
        None => ui::info(
            ctx.json,
            &format!("{app_id} is not running; streaming all device logs. Ctrl+C to stop."),
        ),
    }
    adb.logcat(&device, pid.as_deref())?;
    Ok(())
}

fn clean(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    ui::info(ctx.json, "Cleaning…");
    run_gradle(ctx, &project, "clean", &[])?;
    ui::ok(ctx.json, "Clean complete");
    if ctx.json {
        ui::print_json(&json!({ "success": true }));
    }
    Ok(())
}

fn deep_clean(ctx: &Ctx, yes: bool) -> Result<()> {
    let project = ctx.project()?;
    let root = PathBuf::from(&project.root);

    if !yes {
        if ctx.json || !std::io::stdin().is_terminal() {
            bail!("Refusing to deep-clean without confirmation; pass --yes (or -y).");
        }
        let confirmed = Confirm::new(&format!(
            "Delete .gradle and all build/ directories under {}?",
            root.display()
        ))
        .with_default(false)
        .prompt()?;
        if !confirmed {
            ui::info(ctx.json, "Aborted.");
            return Ok(());
        }
    }

    // Stop daemons first so files aren't locked.
    let _ = ctx.gradle(&project).and_then(|g| g.stop_daemons());

    let mut removed: Vec<String> = Vec::new();
    let dot_gradle = root.join(".gradle");
    if dot_gradle.exists() {
        std::fs::remove_dir_all(&dot_gradle)
            .with_context(|| format!("removing {}", dot_gradle.display()))?;
        removed.push(dot_gradle.to_string_lossy().to_string());
    }
    delete_build_dirs(&root, &mut removed)?;

    for path in &removed {
        ui::info(ctx.json, &format!("  removed {}", path.dimmed()));
    }
    ui::ok(
        ctx.json,
        &format!("Deep clean complete ({} paths removed)", removed.len()),
    );
    if ctx.json {
        ui::print_json(&json!({ "success": true, "removed": removed }));
    }
    Ok(())
}

fn stop(ctx: &Ctx) -> Result<()> {
    let (adb, device, app_id, _project) = device_and_app(ctx)?;
    adb.stop_app(&device, &app_id)?;
    ui::ok(ctx.json, &format!("Stopped {app_id}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "package": app_id, "device": device }));
    }
    Ok(())
}

fn clear_data(ctx: &Ctx) -> Result<()> {
    let (adb, device, app_id, _project) = device_and_app(ctx)?;
    adb.clear_data(&device, &app_id)?;
    ui::ok(ctx.json, &format!("Cleared data for {app_id}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "package": app_id, "device": device }));
    }
    Ok(())
}

fn restart(ctx: &Ctx) -> Result<()> {
    let (adb, device, app_id, project) = device_and_app(ctx)?;
    adb.stop_app(&device, &app_id)?;
    let component = project
        .launch_activity
        .clone()
        .ok_or_else(|| anyhow!("Could not determine the launcher activity from the manifest"))?;
    adb.start_activity(&device, &component)?;
    ui::ok(ctx.json, &format!("Restarted {app_id}"));
    if ctx.json {
        ui::print_json(
            &json!({ "success": true, "package": app_id, "component": component, "device": device }),
        );
    }
    Ok(())
}

fn devices(ctx: &Ctx) -> Result<()> {
    let adb = Adb::new()?;
    let list = adb.devices()?;
    if ctx.json {
        ui::print_json(&json!({ "devices": list }));
    } else {
        println!("{}", "Connected devices".bold().underline().yellow());
        for d in &list {
            println!("  {}", d.green());
        }
    }
    Ok(())
}

fn screenshot(ctx: &Ctx, output: Option<PathBuf>) -> Result<()> {
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    let path = adb.screenshot(&device, output)?;
    ui::ok(ctx.json, &format!("Screenshot saved to {}", path.display()));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "file": path.to_string_lossy() }));
    }
    Ok(())
}

fn record(ctx: &Ctx, output: Option<PathBuf>) -> Result<()> {
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    ui::info(ctx.json, "Recording… press Ctrl+C to stop.");
    let path = adb.record_screen(&device, output)?;
    ui::ok(ctx.json, &format!("Recording saved to {}", path.display()));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "file": path.to_string_lossy() }));
    }
    Ok(())
}

// ---- helpers -----------------------------------------------------------

/// Resolve the project + a device + the app id for app-lifecycle commands.
fn device_and_app(ctx: &Ctx) -> Result<(Adb, String, String, AndroidProject)> {
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?.to_string();
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    Ok((adb, device, app_id, project))
}

/// Run a Gradle task, mapping a non-zero exit into an error. In JSON mode we
/// pass `-q` so Gradle's chatter doesn't pollute stdout before the result line.
fn run_gradle(ctx: &Ctx, project: &AndroidProject, task: &str, extra: &[&str]) -> Result<()> {
    let gradle = ctx.gradle(project)?;
    let mut args: Vec<&str> = Vec::new();
    if ctx.json {
        args.push("-q");
    }
    args.extend_from_slice(extra);
    let status = gradle.run_task(task, &args)?;
    if !status.success() {
        bail!("Gradle task `{task}` failed");
    }
    Ok(())
}

/// Recursively delete every directory named `build`, skipping hidden dirs and
/// not descending into a `build` dir we are about to remove.
fn delete_build_dirs(dir: &Path, removed: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "build" {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("removing {}", path.display()))?;
            removed.push(path.to_string_lossy().to_string());
        } else if !name.starts_with('.') {
            delete_build_dirs(&path, removed)?;
        }
    }
    Ok(())
}
