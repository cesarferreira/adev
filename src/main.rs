//! `droid` — a project-aware Android developer CLI built on `androkit`.
//!
//! Discovers your repo's modules, variants, applicationId, and launcher
//! activity, then maps simple verbs (`info`, `install`, `launch`, `test`,
//! `clean`, `deep-clean`, `logs`, …) onto the right Gradle tasks and ADB
//! commands — so you don't have to remember `testDevDebugUnitTest`.

mod cli;
mod commands;
mod ui;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Command};
use colored::*;
use commands::Ctx;
use inquire::{InquireError, Select};
use std::io::IsTerminal;
use std::process::exit;

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(err) = run(cli) {
        // A Ctrl+C / Esc at an interactive prompt is a clean exit, not an error.
        if let Some(InquireError::OperationInterrupted | InquireError::OperationCanceled) =
            err.downcast_ref::<InquireError>()
        {
            exit(0);
        }
        if json {
            eprintln!("{}", serde_json::json!({ "error": err.to_string() }));
        } else {
            eprintln!("{} {}", "Error:".red().bold(), err);
        }
        exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let ctx = Ctx {
        json: cli.json,
        device: cli.device,
        variant: cli.variant,
        module: cli.module,
    };
    let command = match cli.command {
        Some(c) => c,
        None => pick_command(&ctx)?,
    };
    commands::run(&ctx, command)
}

/// No subcommand → a fuzzy action picker (TTY only).
fn pick_command(ctx: &Ctx) -> Result<Command> {
    if ctx.json || !std::io::stdin().is_terminal() {
        bail!("No command given. Run `droid --help` to see available commands.");
    }
    let options = vec![
        "info",
        "install",
        "launch",
        "test",
        "logs",
        "clean",
        "deep-clean",
        "devices",
        "screenshot",
    ];
    let choice = Select::new("What do you want to do?", options).prompt()?;
    Ok(match choice {
        "info" => Command::Info,
        "install" => Command::Install { variant: None },
        "launch" => Command::Launch,
        "test" => Command::Test { fresh: false },
        "logs" => Command::Logs,
        "clean" => Command::Clean,
        "deep-clean" => Command::DeepClean { yes: false },
        "devices" => Command::Devices,
        "screenshot" => Command::Screenshot { output: None },
        _ => unreachable!("unknown menu option"),
    })
}
