//! Command implementations. Each maps a CLI command to `androkit` calls, then
//! renders the result (human text or JSON) via [`crate::ui`].

use crate::cli::{CacheCommand, Command};
use crate::ui;
use androkit::adb::Adb;
use androkit::error::{anyhow, bail, Context, Result};
use androkit::exec;
use androkit::gradle::Gradle;
use androkit::model::{capitalize, AndroidProject};
use androkit::project;
use colored::*;
use inquire::Confirm;
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

/// Shared flags threaded through every command.
pub struct Ctx {
    pub json: bool,
    pub device: Option<String>,
    pub variant: Option<String>,
    pub module: Option<String>,
    pub dry_run: bool,
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
        Command::Info { refresh } => info(ctx, refresh),
        Command::Install { variant } => install(ctx, variant),
        Command::Build { variant } => build(ctx, variant),
        Command::Launch => launch(ctx),
        Command::Run {
            variant,
            fresh,
            clear_data,
            restart,
            logs,
        } => run_app(ctx, variant, fresh, clear_data, restart, logs),
        Command::Test { fresh } => test(ctx, fresh),
        Command::ConnectedTest { fresh } => connected_test(ctx, fresh),
        Command::Logs {
            clear,
            tag,
            level,
            crashes,
        } => logs(ctx, clear, tag, level, crashes),
        Command::Clean => clean(ctx),
        Command::DeepClean { yes } => deep_clean(ctx, yes),
        Command::Stop => stop(ctx),
        Command::ClearData => clear_data(ctx),
        Command::Uninstall { yes } => uninstall(ctx, yes),
        Command::Grant { permissions } => grant(ctx, permissions),
        Command::Revoke { permissions } => revoke(ctx, permissions),
        Command::Restart => restart(ctx),
        Command::Devices { verbose, health } => devices(ctx, verbose, health),
        Command::Health => health(ctx),
        Command::Doctor => doctor(ctx),
        Command::Open { url } => open(ctx, url),
        Command::Cache { command } => cache(ctx, command),
        Command::Screenshot { output } => screenshot(ctx, output),
        Command::Record { output } => record(ctx, output),
    }
}

fn info(ctx: &Ctx, refresh: bool) -> Result<()> {
    if refresh {
        project::clear_cache().with_context(|| "clearing discovery cache before refresh")?;
    }
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

fn build(ctx: &Ctx, positional_variant: Option<String>) -> Result<()> {
    let project = ctx.project()?;
    let variant = ui::resolve_variant(
        &project,
        positional_variant.as_deref().or(ctx.variant.as_deref()),
    )?;
    let task = ctx.scoped(&project.assemble_task(&variant));
    if emit_gradle_dry_run(
        ctx,
        &project,
        "build",
        &task,
        &[],
        json!({ "variant": variant, "task": task }),
    ) {
        return Ok(());
    }

    ui::info(
        ctx.json,
        &format!("Building {} ({})…", variant.bold(), task.dimmed()),
    );
    run_gradle(ctx, &project, &task, &[])?;
    let apks = built_apks(&project, ctx.module.as_deref(), &variant)?;
    ui::ok(ctx.json, &format!("Built {variant}"));
    if ctx.json {
        let apk_values: Vec<String> = apks
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        ui::print_json(
            &json!({ "success": true, "variant": variant, "task": task, "apks": apk_values }),
        );
    } else if !apks.is_empty() {
        for apk in apks {
            ui::info(
                ctx.json,
                &format!("  APK {}", apk.display().to_string().dimmed()),
            );
        }
    }
    Ok(())
}

fn run_app(
    ctx: &Ctx,
    positional_variant: Option<String>,
    fresh: bool,
    clear_data: bool,
    restart: bool,
    attach_logs: bool,
) -> Result<()> {
    let project = ctx.project()?;
    let variant = ui::resolve_variant(
        &project,
        positional_variant.as_deref().or(ctx.variant.as_deref()),
    )?;
    let task = ctx.scoped(&project.install_task(&variant));
    let app_id = ui::require_application_id(&project)?.to_string();
    let component = project
        .launch_activity
        .clone()
        .ok_or_else(|| anyhow!("Could not determine the launcher activity from the manifest"))?;
    let extra = fresh_args(fresh);
    let device = planned_device(ctx);

    if ctx.dry_run {
        let mut steps = vec![gradle_dry_run_value(
            &project,
            "install",
            &task,
            &extra,
            json!({ "variant": variant, "task": task, "fresh": fresh }),
        )];
        if clear_data {
            steps.push(adb_dry_run_value(
                "clear-data",
                &adb_args(&device, &["shell", "pm", "clear", &app_id]),
                json!({ "package": app_id }),
            ));
        } else if restart {
            steps.push(adb_dry_run_value(
                "stop",
                &adb_args(&device, &["shell", "am", "force-stop", &app_id]),
                json!({ "package": app_id }),
            ));
        }
        steps.push(adb_dry_run_value(
            "launch",
            &adb_args(&device, &["shell", "am", "start", "-n", &component]),
            json!({ "component": component }),
        ));
        if attach_logs {
            steps.push(adb_dry_run_value(
                "logs",
                &logcat_stream_args(&device, None, &LogOptions::default())?,
                json!({ "package": app_id }),
            ));
        }
        emit_workflow_dry_run(ctx, "run", steps);
        return Ok(());
    }

    ui::info(
        ctx.json,
        &format!("Installing {} ({})…", variant.bold(), task.dimmed()),
    );
    run_gradle(ctx, &project, &task, &extra)?;

    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    if clear_data {
        adb.clear_data(&device, &app_id)?;
    } else if restart {
        adb.stop_app(&device, &app_id)?;
    }
    adb.start_activity(&device, &component)?;
    ui::ok(ctx.json, &format!("Ran {app_id} ({variant})"));
    if ctx.json && !attach_logs {
        ui::print_json(&json!({
            "success": true,
            "variant": variant,
            "task": task,
            "package": app_id,
            "component": component,
            "device": device
        }));
    }
    if attach_logs {
        let pid = adb.pid_of(&device, &app_id)?;
        run_logcat_stream(&logcat_stream_args(
            &device,
            pid.as_deref(),
            &LogOptions::default(),
        )?)?;
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
    if emit_gradle_dry_run(
        ctx,
        &project,
        "install",
        &task,
        &[],
        json!({ "variant": variant, "task": task }),
    ) {
        return Ok(());
    }
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
    let device = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "launch",
        &adb_args(&device, &["shell", "am", "start", "-n", &component]),
        json!({ "component": component }),
    ) {
        return Ok(());
    }
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
    let extra = fresh_args(fresh);
    if emit_gradle_dry_run(
        ctx,
        &project,
        "test",
        &task,
        &extra,
        json!({ "variant": variant, "task": task, "fresh": fresh }),
    ) {
        return Ok(());
    }
    ui::info(
        ctx.json,
        &format!("Testing {} ({})…", variant.bold(), task.dimmed()),
    );
    run_gradle(ctx, &project, &task, &extra)?;
    ui::ok(ctx.json, &format!("Tests passed: {variant}"));
    if ctx.json {
        ui::print_json(
            &json!({ "success": true, "variant": variant, "task": task, "fresh": fresh }),
        );
    }
    Ok(())
}

fn connected_test(ctx: &Ctx, fresh: bool) -> Result<()> {
    let project = ctx.project()?;
    let variant = ui::resolve_variant(&project, ctx.variant.as_deref())?;
    let task = ctx.scoped(&connected_test_task(&variant));
    let extra = fresh_args(fresh);
    if emit_gradle_dry_run(
        ctx,
        &project,
        "connected-test",
        &task,
        &extra,
        json!({ "variant": variant, "task": task, "fresh": fresh }),
    ) {
        return Ok(());
    }
    ui::info(
        ctx.json,
        &format!(
            "Running connected tests for {} ({})…",
            variant.bold(),
            task.dimmed()
        ),
    );
    run_gradle(ctx, &project, &task, &extra)?;
    ui::ok(ctx.json, &format!("Connected tests passed: {variant}"));
    if ctx.json {
        ui::print_json(
            &json!({ "success": true, "variant": variant, "task": task, "fresh": fresh }),
        );
    }
    Ok(())
}

fn logs(
    ctx: &Ctx,
    clear: bool,
    tag: Vec<String>,
    level: Option<String>,
    crashes: bool,
) -> Result<()> {
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?;
    let options = LogOptions {
        clear,
        tags: tag,
        level,
        crashes,
    };
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "logs",
        &logcat_stream_args(&planned, None, &options)?,
        json!({ "package": app_id, "clear": clear }),
    ) {
        return Ok(());
    }

    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    if options.clear {
        let args = logcat_clear_args(&device);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = adb.run(&refs)?;
        if !output.status.success() {
            bail!("Failed to clear logcat");
        }
    }
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
    run_logcat_stream(&logcat_stream_args(&device, pid.as_deref(), &options)?)?;
    Ok(())
}

fn uninstall(ctx: &Ctx, yes: bool) -> Result<()> {
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?.to_string();
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "uninstall",
        &adb_args(&planned, &["uninstall", &app_id]),
        json!({ "package": app_id }),
    ) {
        return Ok(());
    }

    if !yes {
        if ctx.json || !std::io::stdin().is_terminal() {
            bail!("Refusing to uninstall without confirmation; pass --yes (or -y).");
        }
        let confirmed = Confirm::new(&format!("Uninstall {app_id}?"))
            .with_default(false)
            .prompt()?;
        if !confirmed {
            ui::info(ctx.json, "Aborted.");
            return Ok(());
        }
    }

    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    adb.uninstall(&device, &app_id)?;
    ui::ok(ctx.json, &format!("Uninstalled {app_id}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "package": app_id, "device": device }));
    }
    Ok(())
}

fn grant(ctx: &Ctx, permissions: Vec<String>) -> Result<()> {
    let (adb, device, app_id, _project) = match dry_run_permissions(ctx, "grant", &permissions)? {
        Some(()) => return Ok(()),
        None => device_and_app(ctx)?,
    };
    let refs = permission_refs(&permissions);
    adb.grant(&device, &app_id, &refs)?;
    ui::ok(
        ctx.json,
        &format!("Granted {} permission(s)", permissions.len()),
    );
    if ctx.json {
        ui::print_json(
            &json!({ "success": true, "package": app_id, "device": device, "permissions": permissions }),
        );
    }
    Ok(())
}

fn revoke(ctx: &Ctx, permissions: Vec<String>) -> Result<()> {
    let (adb, device, app_id, _project) = match dry_run_permissions(ctx, "revoke", &permissions)? {
        Some(()) => return Ok(()),
        None => device_and_app(ctx)?,
    };
    let refs = permission_refs(&permissions);
    adb.revoke(&device, &app_id, &refs)?;
    ui::ok(
        ctx.json,
        &format!("Revoked {} permission(s)", permissions.len()),
    );
    if ctx.json {
        ui::print_json(
            &json!({ "success": true, "package": app_id, "device": device, "permissions": permissions }),
        );
    }
    Ok(())
}

fn clean(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    if emit_gradle_dry_run(
        ctx,
        &project,
        "clean",
        "clean",
        &[],
        json!({ "task": "clean" }),
    ) {
        return Ok(());
    }
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
    if ctx.dry_run {
        let targets = deep_clean_targets(&root)?;
        emit_file_dry_run(
            ctx,
            "deep-clean",
            &root,
            json!({ "targets": targets, "stop_gradle_daemons": true }),
        );
        return Ok(());
    }

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
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?.to_string();
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "stop",
        &adb_args(&planned, &["shell", "am", "force-stop", &app_id]),
        json!({ "package": app_id }),
    ) {
        return Ok(());
    }
    let (adb, device, app_id, _project) = device_and_app(ctx)?;
    adb.stop_app(&device, &app_id)?;
    ui::ok(ctx.json, &format!("Stopped {app_id}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "package": app_id, "device": device }));
    }
    Ok(())
}

fn clear_data(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?.to_string();
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "clear-data",
        &adb_args(&planned, &["shell", "pm", "clear", &app_id]),
        json!({ "package": app_id }),
    ) {
        return Ok(());
    }
    let (adb, device, app_id, _project) = device_and_app(ctx)?;
    adb.clear_data(&device, &app_id)?;
    ui::ok(ctx.json, &format!("Cleared data for {app_id}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "package": app_id, "device": device }));
    }
    Ok(())
}

fn restart(ctx: &Ctx) -> Result<()> {
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?.to_string();
    let component = project
        .launch_activity
        .clone()
        .ok_or_else(|| anyhow!("Could not determine the launcher activity from the manifest"))?;
    let planned = planned_device(ctx);
    if ctx.dry_run {
        emit_workflow_dry_run(
            ctx,
            "restart",
            vec![
                adb_dry_run_value(
                    "stop",
                    &adb_args(&planned, &["shell", "am", "force-stop", &app_id]),
                    json!({ "package": app_id }),
                ),
                adb_dry_run_value(
                    "launch",
                    &adb_args(&planned, &["shell", "am", "start", "-n", &component]),
                    json!({ "component": component }),
                ),
            ],
        );
        return Ok(());
    }
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

fn devices(ctx: &Ctx, verbose: bool, health: bool) -> Result<()> {
    if emit_adb_dry_run(
        ctx,
        "devices",
        &["devices".to_string(), "-l".to_string()],
        json!({ "verbose": verbose, "health": health }),
    ) {
        return Ok(());
    }
    let adb = Adb::new()?;
    let list = adb.devices()?;
    if ctx.json && !verbose && !health {
        ui::print_json(&json!({ "devices": list }));
    } else if ctx.json {
        let mut values = Vec::new();
        for serial in &list {
            let mut item = json!({ "serial": serial });
            if verbose {
                item["info"] = serde_json::to_value(adb.device_info(serial)?)?;
            }
            if health {
                item["health"] = serde_json::to_value(adb.device_health(serial)?)?;
            }
            values.push(item);
        }
        ui::print_json(&json!({ "devices": values }));
    } else {
        println!("{}", "Connected devices".bold().underline().yellow());
        for d in &list {
            println!("  {}", d.green());
            if verbose {
                let info = adb.device_info(d)?;
                if let Some(model) = info.model {
                    println!("    model: {}", model.dimmed());
                }
                if let Some(version) = info.android_version {
                    println!("    android: {}", version.dimmed());
                }
            }
            if health {
                let snapshot = adb.device_health(d)?;
                if let Some(level) = snapshot.battery.level {
                    println!("    battery: {}%", level.dimmed());
                }
                if let Some(storage) = snapshot.storage {
                    println!(
                        "    storage: {} GB free / {} GB total",
                        storage.free_gb.to_string().dimmed(),
                        storage.total_gb.to_string().dimmed()
                    );
                }
            }
        }
    }
    Ok(())
}

fn health(ctx: &Ctx) -> Result<()> {
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "health",
        &adb_args(&planned, &["shell", "dumpsys", "battery"]),
        json!({ "device": planned }),
    ) {
        return Ok(());
    }
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    let snapshot = adb.device_health(&device)?;
    if ctx.json {
        ui::print_json(&serde_json::to_value(snapshot)?);
    } else {
        println!("{}", "Device health".bold().underline().yellow());
        println!("{:<12}: {}", "Device".cyan(), snapshot.device.green());
        if let Some(level) = snapshot.battery.level {
            println!("{:<12}: {}%", "Battery".cyan(), level.green());
        }
        if let Some(storage) = snapshot.storage {
            println!(
                "{:<12}: {} GB free / {} GB total",
                "Storage".cyan(),
                storage.free_gb,
                storage.total_gb
            );
        }
        println!("{:<12}: {} GB free", "RAM".cyan(), snapshot.ram.free_gb);
        if let Some(ip) = snapshot.network.ip {
            println!("{:<12}: {}", "IP".cyan(), ip.green());
        }
        if let Some(ssid) = snapshot.network.ssid {
            println!("{:<12}: {}", "Wi-Fi".cyan(), ssid.green());
        }
    }
    Ok(())
}

fn doctor(ctx: &Ctx) -> Result<()> {
    let checks = doctor_checks()?;
    let success = checks
        .iter()
        .all(|check| check["ok"].as_bool() == Some(true));
    if ctx.json {
        ui::print_json(&json!({ "success": success, "checks": checks }));
    } else {
        println!("{}", "Doctor".bold().underline().yellow());
        for check in &checks {
            let marker = if check["ok"].as_bool() == Some(true) {
                "✓".green().bold()
            } else {
                "✗".red().bold()
            };
            let name = check["name"].as_str().unwrap_or("check");
            let detail = check["detail"].as_str().unwrap_or("");
            println!("{} {:<18} {}", marker, name.cyan(), detail.dimmed());
        }
    }
    Ok(())
}

fn open(ctx: &Ctx, url: String) -> Result<()> {
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "open",
        &adb_args(
            &planned,
            &[
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                &url,
            ],
        ),
        json!({ "url": url }),
    ) {
        return Ok(());
    }
    let adb = Adb::new()?;
    let device = ui::select_device(&adb, ctx.device.as_deref(), ctx.json)?;
    adb.launch_url(&device, &url)?;
    ui::ok(ctx.json, &format!("Opened {url}"));
    if ctx.json {
        ui::print_json(&json!({ "success": true, "url": url, "device": device }));
    }
    Ok(())
}

fn cache(ctx: &Ctx, command: CacheCommand) -> Result<()> {
    match command {
        CacheCommand::Clear => {
            if emit_file_dry_run(
                ctx,
                "cache-clear",
                &std::env::current_dir()?,
                json!({ "cache": "androkit discovery" }),
            ) {
                return Ok(());
            }
            project::clear_cache().with_context(|| "clearing discovery cache")?;
            ui::ok(ctx.json, "Cleared discovery cache");
            if ctx.json {
                ui::print_json(&json!({ "success": true }));
            }
        }
    }
    Ok(())
}

fn screenshot(ctx: &Ctx, output: Option<PathBuf>) -> Result<()> {
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "screenshot",
        &adb_args(
            &planned,
            &["shell", "screencap", "-p", "/sdcard/screen.png"],
        ),
        json!({ "output": output }),
    ) {
        return Ok(());
    }
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
    let planned = planned_device(ctx);
    if emit_adb_dry_run(
        ctx,
        "record",
        &adb_args(&planned, &["shell", "screenrecord", "/sdcard/demo.mp4"]),
        json!({ "output": output }),
    ) {
        return Ok(());
    }
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

fn connected_test_task(variant: &str) -> String {
    format!("connected{}AndroidTest", capitalize(variant))
}

fn fresh_args(fresh: bool) -> Vec<&'static str> {
    if fresh {
        vec!["--rerun-tasks", "--no-build-cache"]
    } else {
        Vec::new()
    }
}

fn module_dir_for(project: &AndroidProject, explicit_module: Option<&str>) -> Result<String> {
    let module_path = explicit_module
        .or(project.app_module.as_deref())
        .or_else(|| {
            project
                .modules
                .iter()
                .find(|m| m.is_application)
                .map(|m| m.path.as_str())
        })
        .ok_or_else(|| anyhow!("Could not determine an Android application module"))?;

    project
        .modules
        .iter()
        .find(|m| m.path == module_path)
        .map(|m| m.dir.clone())
        .ok_or_else(|| anyhow!("Could not find module {module_path} in discovered project"))
}

fn built_apks(
    project: &AndroidProject,
    explicit_module: Option<&str>,
    variant: &str,
) -> Result<Vec<PathBuf>> {
    let module_dir = module_dir_for(project, explicit_module)?;
    let outputs = Path::new(&project.root)
        .join(module_dir)
        .join("build")
        .join("outputs")
        .join("apk");
    if !outputs.exists() {
        return Ok(Vec::new());
    }

    let variant_tokens = variant_tokens(variant);
    let mut apks = Vec::new();
    collect_apks(&outputs, &variant_tokens, &mut apks)?;
    apks.sort();
    Ok(apks)
}

fn collect_apks(dir: &Path, variant_tokens: &[String], apks: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_apks(&path, variant_tokens, apks)?;
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("apk") {
            continue;
        }
        let haystack = path.to_string_lossy().to_lowercase();
        if variant_tokens.iter().all(|token| haystack.contains(token)) {
            apks.push(path);
        }
    }
    Ok(())
}

fn variant_tokens(variant: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in variant.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    if tokens.is_empty() {
        tokens.push(variant.to_lowercase());
    }
    tokens
}

#[derive(Debug, Clone, Default)]
struct LogOptions {
    clear: bool,
    tags: Vec<String>,
    level: Option<String>,
    crashes: bool,
}

fn logcat_clear_args(device: &str) -> Vec<String> {
    vec![
        "-s".to_string(),
        device.to_string(),
        "logcat".to_string(),
        "-c".to_string(),
    ]
}

fn logcat_stream_args(
    device: &str,
    pid: Option<&str>,
    options: &LogOptions,
) -> Result<Vec<String>> {
    let mut args = vec!["-s".to_string(), device.to_string(), "logcat".to_string()];
    if let Some(pid) = pid {
        args.push(format!("--pid={pid}"));
    }

    let level = options
        .level
        .as_deref()
        .map(normalize_log_level)
        .transpose()?;

    let mut filters = Vec::new();
    if !options.tags.is_empty() {
        let tag_level = level.as_deref().unwrap_or("V");
        filters.extend(options.tags.iter().map(|tag| format!("{tag}:{tag_level}")));
    } else if let Some(level) = &level {
        filters.push(format!("*:{level}"));
    }

    if options.crashes {
        filters.extend([
            "AndroidRuntime:E".to_string(),
            "DEBUG:E".to_string(),
            "libc:F".to_string(),
        ]);
    }

    if !filters.is_empty() {
        args.extend(filters);
        args.push("*:S".to_string());
    }

    Ok(args)
}

fn normalize_log_level(level: &str) -> Result<String> {
    let normalized = level.trim().to_uppercase();
    match normalized.as_str() {
        "V" | "D" | "I" | "W" | "E" | "F" | "S" => Ok(normalized),
        _ => bail!("Invalid log level `{level}`. Use one of V, D, I, W, E, F, S."),
    }
}

fn permission_refs(permissions: &[String]) -> Vec<&str> {
    permissions.iter().map(String::as_str).collect()
}

fn dry_run_payload(
    kind: &str,
    cwd: &Path,
    program: &str,
    args: &[String],
    metadata: Value,
) -> Value {
    json!({
        "dry_run": true,
        "kind": kind,
        "cwd": cwd.to_string_lossy(),
        "program": program,
        "args": args,
        "metadata": metadata,
    })
}

fn planned_device(ctx: &Ctx) -> String {
    ctx.device.clone().unwrap_or_else(|| "<device>".to_string())
}

fn adb_args(device: &str, tail: &[&str]) -> Vec<String> {
    let mut args = vec!["-s".to_string(), device.to_string()];
    args.extend(tail.iter().map(|arg| arg.to_string()));
    args
}

fn gradle_dry_run_value(
    project: &AndroidProject,
    kind: &str,
    task: &str,
    extra: &[&str],
    metadata: Value,
) -> Value {
    let mut args = vec![task.to_string()];
    args.extend(extra.iter().map(|arg| arg.to_string()));
    dry_run_payload(kind, Path::new(&project.root), "./gradlew", &args, metadata)
}

fn adb_dry_run_value(kind: &str, args: &[String], metadata: Value) -> Value {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    dry_run_payload(kind, &cwd, "adb", args, metadata)
}

fn emit_gradle_dry_run(
    ctx: &Ctx,
    project: &AndroidProject,
    kind: &str,
    task: &str,
    extra: &[&str],
    metadata: Value,
) -> bool {
    if !ctx.dry_run {
        return false;
    }
    print_dry_run_value(
        ctx,
        &gradle_dry_run_value(project, kind, task, extra, metadata),
    );
    true
}

fn emit_adb_dry_run(ctx: &Ctx, kind: &str, args: &[String], metadata: Value) -> bool {
    if !ctx.dry_run {
        return false;
    }
    print_dry_run_value(ctx, &adb_dry_run_value(kind, args, metadata));
    true
}

fn emit_file_dry_run(ctx: &Ctx, kind: &str, cwd: &Path, metadata: Value) -> bool {
    if !ctx.dry_run {
        return false;
    }
    let args = vec![kind.to_string()];
    print_dry_run_value(
        ctx,
        &dry_run_payload("file-system", cwd, "adev", &args, metadata),
    );
    true
}

fn emit_workflow_dry_run(ctx: &Ctx, name: &str, steps: Vec<Value>) {
    let value = json!({
        "dry_run": true,
        "kind": "workflow",
        "name": name,
        "steps": steps,
    });
    print_dry_run_value(ctx, &value);
}

fn print_dry_run_value(ctx: &Ctx, value: &Value) {
    if ctx.json {
        ui::print_json(value);
        return;
    }

    if value["kind"].as_str() == Some("workflow") {
        println!("{} {}", "Dry run:".yellow().bold(), value["name"]);
        if let Some(steps) = value["steps"].as_array() {
            for step in steps {
                print_dry_run_command(step);
            }
        }
    } else {
        print_dry_run_command(value);
    }
}

fn print_dry_run_command(value: &Value) {
    let program = value["program"].as_str().unwrap_or("adev");
    let args = value["args"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let cwd = value["cwd"].as_str().unwrap_or(".");
    println!(
        "{} ({}) {} {}",
        "Dry run:".yellow().bold(),
        cwd.dimmed(),
        program.green(),
        args
    );
}

fn dry_run_permissions(ctx: &Ctx, action: &str, permissions: &[String]) -> Result<Option<()>> {
    if !ctx.dry_run {
        return Ok(None);
    }
    let project = ctx.project()?;
    let app_id = ui::require_application_id(&project)?.to_string();
    let device = planned_device(ctx);
    let mut steps = Vec::new();
    for permission in permissions {
        steps.push(adb_dry_run_value(
            action,
            &adb_args(&device, &["shell", "pm", action, &app_id, permission]),
            json!({ "package": app_id, "permission": permission }),
        ));
    }
    emit_workflow_dry_run(ctx, action, steps);
    Ok(Some(()))
}

fn run_logcat_stream(args: &[String]) -> Result<()> {
    let adb = exec::find_program("adb")?;
    let _status = ProcessCommand::new(adb).args(args).status()?;
    Ok(())
}

fn deep_clean_targets(root: &Path) -> Result<Vec<String>> {
    let mut targets = Vec::new();
    let dot_gradle = root.join(".gradle");
    if dot_gradle.exists() {
        targets.push(dot_gradle.to_string_lossy().to_string());
    }
    collect_build_dir_targets(root, &mut targets)?;
    Ok(targets)
}

fn collect_build_dir_targets(dir: &Path, targets: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "build" {
            targets.push(path.to_string_lossy().to_string());
        } else if !name.starts_with('.') {
            collect_build_dir_targets(&path, targets)?;
        }
    }
    Ok(())
}

fn doctor_checks() -> Result<Vec<Value>> {
    let cwd = std::env::current_dir()?;
    let mut checks = Vec::new();

    match exec::find_program("adb") {
        Ok(path) => checks.push(doctor_check("adb", true, &path.display().to_string())),
        Err(err) => checks.push(doctor_check("adb", false, &err.to_string())),
    }

    let project_result = project::discover(&cwd);
    match &project_result {
        Ok(project) => {
            checks.push(doctor_check("project", true, &project.root));
            checks.push(doctor_check(
                "applicationId",
                project.application_id.is_some(),
                project
                    .application_id
                    .as_deref()
                    .unwrap_or("not discovered"),
            ));
            checks.push(doctor_check(
                "launcher",
                project.launch_activity.is_some(),
                project
                    .launch_activity
                    .as_deref()
                    .unwrap_or("not discovered"),
            ));
            checks.push(doctor_check(
                "defaultVariant",
                project.default_variant.is_some(),
                project
                    .default_variant
                    .as_deref()
                    .unwrap_or("not discovered"),
            ));
            match Gradle::at(Path::new(&project.root)) {
                Ok(_) => checks.push(doctor_check("gradle wrapper", true, "found")),
                Err(err) => checks.push(doctor_check("gradle wrapper", false, &err.to_string())),
            }
        }
        Err(err) => checks.push(doctor_check("project", false, &err.to_string())),
    }

    match Adb::new().and_then(|adb| adb.devices()) {
        Ok(devices) => checks.push(doctor_check("devices", true, &devices.join(", "))),
        Err(err) => checks.push(doctor_check("devices", false, &err.to_string())),
    }

    Ok(checks)
}

fn doctor_check(name: &str, ok: bool, detail: &str) -> Value {
    json!({
        "name": name,
        "ok": ok,
        "detail": detail,
    })
}

/// Run a Gradle task, mapping a non-zero exit into an error. In JSON mode we
/// pass `-q` so Gradle's chatter doesn't pollute stdout before the result line.
/// In a terminal we force `--console=rich` so Gradle emits its usual colored
/// output even though it's launched as a child process.
fn run_gradle(ctx: &Ctx, project: &AndroidProject, task: &str, extra: &[&str]) -> Result<()> {
    let gradle = ctx.gradle(project)?;
    let args = gradle_args(ctx.json, std::io::stdout().is_terminal(), extra);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let status = gradle.run_task(task, &arg_refs)?;
    if !status.success() {
        bail!("Gradle task `{task}` failed");
    }
    Ok(())
}

/// Build the extra Gradle CLI args, separated from process spawning so it can be
/// unit-tested. In JSON mode we pass `-q` so Gradle's chatter doesn't pollute
/// stdout before the result line. In a terminal (non-JSON) we force
/// `--console=rich` so Gradle still emits its usual colored output even though
/// it's launched as a child process. Caller-supplied `extra` is always appended.
fn gradle_args(json: bool, is_terminal: bool, extra: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if json {
        args.push("-q".to_string());
    } else if is_terminal {
        args.push("--console=rich".to_string());
    }
    args.extend(extra.iter().map(|s| s.to_string()));
    args
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(module: Option<&str>) -> Ctx {
        Ctx {
            json: false,
            device: None,
            variant: None,
            module: module.map(str::to_string),
            dry_run: false,
        }
    }

    fn project(root: &Path) -> AndroidProject {
        AndroidProject {
            root: root.to_string_lossy().to_string(),
            modules: vec![
                androkit::model::Module {
                    path: ":app".to_string(),
                    dir: "app".to_string(),
                    is_application: true,
                },
                androkit::model::Module {
                    path: ":feature:home".to_string(),
                    dir: "feature/home".to_string(),
                    is_application: false,
                },
            ],
            app_module: Some(":app".to_string()),
            variants: vec![androkit::model::Variant {
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
    fn scoped_without_module_is_identity() {
        assert_eq!(ctx(None).scoped("installDevDebug"), "installDevDebug");
    }

    #[test]
    fn scoped_with_module_prefixes_task() {
        assert_eq!(
            ctx(Some(":app")).scoped("installDevDebug"),
            ":app:installDevDebug"
        );
    }

    #[test]
    fn scoped_trims_trailing_colon_on_module() {
        // `--module :app:` should not produce a double colon before the task.
        assert_eq!(
            ctx(Some(":app:")).scoped("installDevDebug"),
            ":app:installDevDebug"
        );
    }

    #[test]
    fn gradle_args_json_mode_uses_quiet_and_no_console() {
        let args = gradle_args(true, true, &[]);
        assert_eq!(args, vec!["-q"]);
        // Even when stdout is a terminal, JSON mode must not request rich console.
        assert!(!args.iter().any(|a| a == "--console=rich"));
    }

    #[test]
    fn gradle_args_terminal_non_json_uses_rich_console() {
        assert_eq!(gradle_args(false, true, &[]), vec!["--console=rich"]);
    }

    #[test]
    fn gradle_args_non_terminal_non_json_is_empty() {
        assert!(gradle_args(false, false, &[]).is_empty());
    }

    #[test]
    fn gradle_args_appends_extra_after_console_flag() {
        let extra = ["--rerun-tasks", "--no-build-cache"];
        assert_eq!(
            gradle_args(false, true, &extra),
            vec!["--console=rich", "--rerun-tasks", "--no-build-cache"]
        );
    }

    #[test]
    fn gradle_args_json_mode_still_appends_extra() {
        let extra = ["--rerun-tasks", "--no-build-cache"];
        assert_eq!(
            gradle_args(true, false, &extra),
            vec!["-q", "--rerun-tasks", "--no-build-cache"]
        );
    }

    #[test]
    fn connected_test_task_camel_cases_variant() {
        assert_eq!(
            connected_test_task("devDebug"),
            "connectedDevDebugAndroidTest"
        );
        assert_eq!(connected_test_task("debug"), "connectedDebugAndroidTest");
    }

    #[test]
    fn fresh_gradle_extra_args_can_be_reused_by_test_commands() {
        assert_eq!(fresh_args(true), vec!["--rerun-tasks", "--no-build-cache"]);
        assert!(fresh_args(false).is_empty());
    }

    #[test]
    fn module_dir_uses_explicit_module_or_project_app_module() {
        let root = Path::new("/tmp/sample");
        let project = project(root);
        assert_eq!(
            module_dir_for(&project, Some(":feature:home")).unwrap(),
            "feature/home"
        );
        assert_eq!(module_dir_for(&project, None).unwrap(), "app");
    }

    #[test]
    fn built_apk_search_returns_existing_apks_under_outputs_apk() {
        let root = std::env::temp_dir().join(format!("adev-test-apks-{}", std::process::id()));
        let apk_dir = root.join("app/build/outputs/apk/dev/debug");
        std::fs::create_dir_all(&apk_dir).unwrap();
        let expected = apk_dir.join("app-dev-debug.apk");
        std::fs::write(&expected, b"fake apk").unwrap();
        std::fs::write(apk_dir.join("metadata.json"), b"{}").unwrap();

        let project = project(&root);
        let apks = built_apks(&project, None, "devDebug").unwrap();

        assert_eq!(apks, vec![expected]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn logcat_args_include_pid_level_tags_and_crash_filters() {
        let options = LogOptions {
            clear: true,
            tags: vec!["AndroidRuntime".to_string(), "MyApp".to_string()],
            level: Some("e".to_string()),
            crashes: true,
        };

        assert_eq!(
            logcat_clear_args("emulator-5554"),
            vec!["-s", "emulator-5554", "logcat", "-c"]
        );
        assert_eq!(
            logcat_stream_args("emulator-5554", Some("1234"), &options).unwrap(),
            vec![
                "-s",
                "emulator-5554",
                "logcat",
                "--pid=1234",
                "AndroidRuntime:E",
                "MyApp:E",
                "AndroidRuntime:E",
                "DEBUG:E",
                "libc:F",
                "*:S",
            ]
        );
    }

    #[test]
    fn logcat_args_default_to_pid_scoped_stream() {
        let options = LogOptions::default();
        assert_eq!(
            logcat_stream_args("device", Some("42"), &options).unwrap(),
            vec!["-s", "device", "logcat", "--pid=42"]
        );
    }

    #[test]
    fn logcat_level_validation_rejects_unknown_values() {
        let options = LogOptions {
            level: Some("nope".to_string()),
            ..Default::default()
        };
        assert!(logcat_stream_args("device", None, &options).is_err());
    }

    #[test]
    fn permission_refs_borrow_permission_strings() {
        let permissions = vec![
            "android.permission.CAMERA".to_string(),
            "android.permission.POST_NOTIFICATIONS".to_string(),
        ];
        assert_eq!(
            permission_refs(&permissions),
            vec![
                "android.permission.CAMERA",
                "android.permission.POST_NOTIFICATIONS"
            ]
        );
    }

    #[test]
    fn dry_run_payload_contains_command_metadata() {
        let payload = dry_run_payload(
            "gradle",
            Path::new("/repo"),
            "./gradlew",
            &["assembleDevDebug".to_string(), "--console=rich".to_string()],
            json!({ "variant": "devDebug" }),
        );

        assert_eq!(
            payload,
            json!({
                "dry_run": true,
                "kind": "gradle",
                "cwd": "/repo",
                "program": "./gradlew",
                "args": ["assembleDevDebug", "--console=rich"],
                "metadata": { "variant": "devDebug" }
            })
        );
    }
}
