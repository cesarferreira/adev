//! Command-line surface for `droid`. Mirrors `dab`'s agent ergonomics:
//! global `--json` / `--device`, plus `--variant` / `--module` for the
//! project-aware commands.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "droid",
    version,
    about = "Project-aware Android developer CLI — knows your repo's test, install, launch & clean commands"
)]
pub struct Cli {
    /// Emit machine-readable JSON (data to stdout, errors as {\"error\":...} to stderr).
    #[arg(long, global = true)]
    pub json: bool,

    /// Target a specific device serial, skipping interactive selection.
    #[arg(long, global = true)]
    pub device: Option<String>,

    /// Build variant to operate on (defaults to the project's resolved default).
    #[arg(long, global = true)]
    pub variant: Option<String>,

    /// Gradle module path to scope to, e.g. `:app` (defaults to the app module).
    #[arg(long, global = true)]
    pub module: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show discovered project structure: modules, variants, applicationId, launch activity, tasks.
    Info,

    /// Install the app onto a device (`./gradlew install<Variant>`).
    Install {
        /// Variant to install (overrides --variant and the default).
        variant: Option<String>,
    },

    /// Launch the discovered launcher activity.
    Launch,

    /// Run unit tests (`./gradlew test<Variant>UnitTest`).
    Test {
        /// Re-run from scratch: `--rerun-tasks --no-build-cache`.
        #[arg(long)]
        fresh: bool,
    },

    /// Stream logcat filtered to this app.
    Logs,

    /// `./gradlew clean`.
    Clean,

    /// Stop Gradle daemons, remove `.gradle`, and delete all `build/` dirs (keeps ~/.gradle).
    DeepClean {
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Force-stop the app on the device.
    Stop,

    /// Clear the app's data and cache.
    ClearData,

    /// Force-stop then relaunch the app.
    Restart,

    /// List connected devices.
    Devices,

    /// Capture a screenshot.
    Screenshot {
        /// Output file or directory.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Record the screen until Ctrl+C.
    Record {
        /// Output file or directory.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
