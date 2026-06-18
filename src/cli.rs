//! Command-line surface for `adev`. Mirrors `dab`'s agent ergonomics:
//! global `--json` / `--device`, plus `--variant` / `--module` for the
//! project-aware commands.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "adev",
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Catches malformed clap definitions (conflicting flags, bad arg specs).
    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_subcommand_is_allowed() {
        // Bare `adev` falls through to the interactive picker, so parsing must succeed.
        let cli = Cli::try_parse_from(["adev"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.json);
    }

    #[test]
    fn global_flags_parse_before_subcommand() {
        let cli = Cli::try_parse_from([
            "adev",
            "--json",
            "--device",
            "emulator-5554",
            "--variant",
            "devDebug",
            "--module",
            ":app",
            "info",
        ])
        .unwrap();
        assert!(cli.json);
        assert_eq!(cli.device.as_deref(), Some("emulator-5554"));
        assert_eq!(cli.variant.as_deref(), Some("devDebug"));
        assert_eq!(cli.module.as_deref(), Some(":app"));
        assert!(matches!(cli.command, Some(Command::Info)));
    }

    #[test]
    fn global_flags_parse_after_subcommand() {
        // `global = true` flags must also be accepted positioned after the subcommand.
        let cli = Cli::try_parse_from(["adev", "info", "--json"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Command::Info)));
    }

    #[test]
    fn install_takes_positional_variant() {
        let cli = Cli::try_parse_from(["adev", "install", "prodRelease"]).unwrap();
        match cli.command {
            Some(Command::Install { variant }) => {
                assert_eq!(variant.as_deref(), Some("prodRelease"))
            }
            other => panic!("expected Install, got {other:?}"),
        }
    }

    #[test]
    fn install_variant_is_optional() {
        let cli = Cli::try_parse_from(["adev", "install"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Install { variant: None })
        ));
    }

    #[test]
    fn test_fresh_flag() {
        let fresh = Cli::try_parse_from(["adev", "test", "--fresh"]).unwrap();
        assert!(matches!(fresh.command, Some(Command::Test { fresh: true })));

        let plain = Cli::try_parse_from(["adev", "test"]).unwrap();
        assert!(matches!(
            plain.command,
            Some(Command::Test { fresh: false })
        ));
    }

    #[test]
    fn deep_clean_yes_short_flag() {
        let cli = Cli::try_parse_from(["adev", "deep-clean", "-y"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::DeepClean { yes: true })
        ));

        let long = Cli::try_parse_from(["adev", "deep-clean", "--yes"]).unwrap();
        assert!(matches!(
            long.command,
            Some(Command::DeepClean { yes: true })
        ));

        let off = Cli::try_parse_from(["adev", "deep-clean"]).unwrap();
        assert!(matches!(
            off.command,
            Some(Command::DeepClean { yes: false })
        ));
    }

    #[test]
    fn screenshot_output_flag() {
        let cli = Cli::try_parse_from(["adev", "screenshot", "--output", "foo.png"]).unwrap();
        match cli.command {
            Some(Command::Screenshot { output }) => {
                assert_eq!(output, Some(PathBuf::from("foo.png")));
            }
            other => panic!("expected Screenshot, got {other:?}"),
        }
    }

    #[test]
    fn unknown_subcommand_is_rejected() {
        assert!(Cli::try_parse_from(["adev", "frobnicate"]).is_err());
    }
}
