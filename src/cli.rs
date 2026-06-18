//! Command-line surface for `adev`. Mirrors `dab`'s agent ergonomics:
//! global `--json` / `--device`, plus `--variant` / `--module` for the
//! project-aware commands.

use clap::{ArgAction, Parser, Subcommand};
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

    /// Print the resolved Gradle/ADB actions without executing them.
    #[arg(long, global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show discovered project structure: modules, variants, applicationId, launch activity, tasks.
    Info {
        /// Bypass the discovery cache before showing project information.
        #[arg(long)]
        refresh: bool,
    },

    /// Install the app onto a device (`./gradlew install<Variant>`).
    Install {
        /// Variant to install (overrides --variant and the default).
        variant: Option<String>,
    },

    /// Build the APK (`./gradlew assemble<Variant>`).
    Build {
        /// Variant to build (overrides --variant and the default).
        variant: Option<String>,
    },

    /// Launch the discovered launcher activity.
    Launch,

    /// Install, optionally reset/restart, launch, and optionally attach logs.
    Run {
        /// Variant to install before launching (overrides --variant and the default).
        variant: Option<String>,

        /// Re-run Gradle work from scratch before installing.
        #[arg(long)]
        fresh: bool,

        /// Clear app data after install and before launch.
        #[arg(long)]
        clear_data: bool,

        /// Force-stop the app before launching it.
        #[arg(long)]
        restart: bool,

        /// Attach filtered logcat after launch.
        #[arg(long)]
        logs: bool,
    },

    /// Run unit tests (`./gradlew test<Variant>UnitTest`).
    Test {
        /// Re-run from scratch: `--rerun-tasks --no-build-cache`.
        #[arg(long)]
        fresh: bool,
    },

    /// Run connected/instrumentation tests (`./gradlew connected<Variant>AndroidTest`).
    ConnectedTest {
        /// Re-run from scratch: `--rerun-tasks --no-build-cache`.
        #[arg(long)]
        fresh: bool,
    },

    /// Stream logcat filtered to this app.
    Logs {
        /// Clear logcat before streaming.
        #[arg(long)]
        clear: bool,

        /// Add a logcat tag filter. May be repeated.
        #[arg(long, action = ArgAction::Append)]
        tag: Vec<String>,

        /// Minimum log level for tag filters, e.g. V, D, I, W, E, F.
        #[arg(long)]
        level: Option<String>,

        /// Show common crash-related logs.
        #[arg(long)]
        crashes: bool,
    },

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

    /// Uninstall the app from the device.
    Uninstall {
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Grant runtime permissions to the app.
    Grant {
        /// Android runtime permissions to grant.
        #[arg(required = true)]
        permissions: Vec<String>,
    },

    /// Revoke runtime permissions from the app.
    Revoke {
        /// Android runtime permissions to revoke.
        #[arg(required = true)]
        permissions: Vec<String>,
    },

    /// Force-stop then relaunch the app.
    Restart,

    /// List connected devices.
    Devices {
        /// Include device properties from getprop.
        #[arg(long)]
        verbose: bool,

        /// Include battery/storage/RAM/network health for each device.
        #[arg(long)]
        health: bool,
    },

    /// Show health for the selected device.
    Health,

    /// Validate local Android project and device prerequisites.
    Doctor,

    /// Open a URL or deep link on the selected device.
    Open {
        /// URL or deep link to open.
        url: String,
    },

    /// Manage adev's discovery cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },

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

#[derive(Subcommand, Debug)]
pub enum CacheCommand {
    /// Clear cached Android project discovery data.
    Clear,
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
        assert!(matches!(cli.command, Some(Command::Info { .. })));
    }

    #[test]
    fn global_flags_parse_after_subcommand() {
        // `global = true` flags must also be accepted positioned after the subcommand.
        let cli = Cli::try_parse_from(["adev", "info", "--json"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Command::Info { .. })));
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

    #[test]
    fn global_dry_run_flag_parses_before_and_after_subcommand() {
        let before = Cli::try_parse_from(["adev", "--dry-run", "install"]).unwrap();
        assert!(before.dry_run);

        let after = Cli::try_parse_from(["adev", "install", "--dry-run"]).unwrap();
        assert!(after.dry_run);
    }

    #[test]
    fn info_refresh_flag_parses() {
        let cli = Cli::try_parse_from(["adev", "info", "--refresh"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Info { refresh: true })));
    }

    #[test]
    fn build_takes_optional_positional_variant() {
        let cli = Cli::try_parse_from(["adev", "build", "prodRelease"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Build { variant: Some(v) }) if v == "prodRelease"
        ));

        let default = Cli::try_parse_from(["adev", "build"]).unwrap();
        assert!(matches!(
            default.command,
            Some(Command::Build { variant: None })
        ));
    }

    #[test]
    fn connected_test_accepts_fresh_flag() {
        let cli = Cli::try_parse_from(["adev", "connected-test", "--fresh"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::ConnectedTest { fresh: true })
        ));
    }

    #[test]
    fn run_accepts_inner_loop_flags() {
        let cli = Cli::try_parse_from([
            "adev",
            "run",
            "devDebug",
            "--fresh",
            "--clear-data",
            "--restart",
            "--logs",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Run {
                variant: Some(v),
                fresh: true,
                clear_data: true,
                restart: true,
                logs: true,
            }) if v == "devDebug"
        ));
    }

    #[test]
    fn logs_accepts_filter_flags() {
        let cli = Cli::try_parse_from([
            "adev",
            "logs",
            "--clear",
            "--tag",
            "AndroidRuntime",
            "--tag",
            "MyApp",
            "--level",
            "E",
            "--crashes",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Logs {
                clear: true,
                tag,
                level: Some(level),
                crashes: true,
            }) if tag == vec!["AndroidRuntime".to_string(), "MyApp".to_string()] && level == "E"
        ));
    }

    #[test]
    fn device_commands_parse() {
        let devices = Cli::try_parse_from(["adev", "devices", "--verbose", "--health"]).unwrap();
        assert!(matches!(
            devices.command,
            Some(Command::Devices {
                verbose: true,
                health: true
            })
        ));

        let health = Cli::try_parse_from(["adev", "health"]).unwrap();
        assert!(matches!(health.command, Some(Command::Health)));
    }

    #[test]
    fn app_management_commands_parse() {
        let open = Cli::try_parse_from(["adev", "open", "myapp://screen"]).unwrap();
        assert!(matches!(
            open.command,
            Some(Command::Open { url }) if url == "myapp://screen"
        ));

        let uninstall = Cli::try_parse_from(["adev", "uninstall", "--yes"]).unwrap();
        assert!(matches!(
            uninstall.command,
            Some(Command::Uninstall { yes: true })
        ));

        let grant = Cli::try_parse_from([
            "adev",
            "grant",
            "android.permission.CAMERA",
            "android.permission.POST_NOTIFICATIONS",
        ])
        .unwrap();
        assert!(matches!(
            grant.command,
            Some(Command::Grant { permissions }) if permissions == vec![
                "android.permission.CAMERA".to_string(),
                "android.permission.POST_NOTIFICATIONS".to_string()
            ]
        ));

        let revoke = Cli::try_parse_from(["adev", "revoke", "android.permission.CAMERA"]).unwrap();
        assert!(matches!(
            revoke.command,
            Some(Command::Revoke { permissions }) if permissions == vec![
                "android.permission.CAMERA".to_string()
            ]
        ));
    }

    #[test]
    fn cache_clear_parses() {
        let cli = Cli::try_parse_from(["adev", "cache", "clear"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Cache {
                command: CacheCommand::Clear
            })
        ));
    }
}
