---
name: adev
description: Project-aware Android developer CLI. Use when you need to build, install, launch, test, clean, or stream logs for an Android (Gradle) project without knowing its variant names, task names, applicationId, or launcher activity. adev discovers all of that automatically. Prefer it over raw ./gradlew and adb when working inside an Android repo.
---

# adev

`adev` is a project-aware Android CLI. Run it from inside an Android (Gradle)
repo; it discovers modules, build variants, `applicationId`, and the launcher
activity, then runs the correct Gradle task or ADB command.

**Always pass `--json` for machine-readable output.** Data goes to **stdout**;
errors go to **stderr** as `{"error": "..."}` with exit code 1. Pass
`--device <serial>`, `--variant <name>`, `--module <:path>`, and `--dry-run`
to avoid any interactive prompt or side effect.

## Discovery first

Start by understanding the project:

```
adev info --json
```

```json
{
  "root": "/path/to/repo",
  "app_module": ":app",
  "application_id": "com.example.sample",
  "launch_activity": "com.example.sample/com.example.sample.ui.MainActivity",
  "default_variant": "devDebug",
  "modules": [
    { "path": ":app", "dir": "app", "is_application": true },
    { "path": ":core", "dir": "core", "is_application": false }
  ],
  "variants": [
    { "name": "devDebug", "build_type": "debug", "flavors": ["dev"] },
    { "name": "devRelease", "build_type": "release", "flavors": ["dev"] },
    { "name": "prodDebug", "build_type": "debug", "flavors": ["prod"] },
    { "name": "prodRelease", "build_type": "release", "flavors": ["prod"] }
  ]
}
```

Use `default_variant` unless the user asks for a specific one. Construct task
names from convention if needed: `install<Variant>`, `test<Variant>UnitTest`,
`assemble<Variant>`.

## Commands

| Command | JSON result (stdout) |
|---|---|
| `adev info --json` | the full project object above |
| `adev info --refresh --json` | clears discovery cache first, then emits the project object |
| `adev build [variant] --json` | `{"success": true, "variant": "...", "task": "assembleDevDebug", "apks": ["..."]}` |
| `adev install [variant] --json` | `{"success": true, "variant": "...", "task": "installDevDebug"}` |
| `adev run [variant] --json --device <s>` | `{"success": true, "variant": "...", "task": "...", "package": "...", "component": "...", "device": "..."}` |
| `adev launch --json --device <s>` | `{"success": true, "component": "...", "device": "..."}` |
| `adev test [--fresh] --json` | `{"success": true, "variant": "...", "task": "...", "fresh": false}` |
| `adev connected-test [--fresh] --json` | `{"success": true, "variant": "...", "task": "connectedDevDebugAndroidTest", "fresh": false}` |
| `adev clean --json` | `{"success": true}` |
| `adev deep-clean -y --json` | `{"success": true, "removed": ["…/.gradle", "…/app/build"]}` |
| `adev stop --json --device <s>` | `{"success": true, "package": "...", "device": "..."}` |
| `adev clear-data --json --device <s>` | `{"success": true, "package": "...", "device": "..."}` |
| `adev restart --json --device <s>` | `{"success": true, "package": "...", "component": "...", "device": "..."}` |
| `adev uninstall -y --json --device <s>` | `{"success": true, "package": "...", "device": "..."}` |
| `adev grant <permission>... --json --device <s>` | `{"success": true, "package": "...", "device": "...", "permissions": ["..."]}` |
| `adev revoke <permission>... --json --device <s>` | `{"success": true, "package": "...", "device": "...", "permissions": ["..."]}` |
| `adev open <url> --json --device <s>` | `{"success": true, "url": "...", "device": "..."}` |
| `adev devices --json` | `{"devices": ["emulator-5554", "1A2B3C"]}` |
| `adev devices --verbose --health --json` | `{"devices": [{"serial": "...", "info": {...}, "health": {...}}]}` |
| `adev health --json --device <s>` | a device health object |
| `adev doctor --json` | `{"success": true|false, "checks": [...]}` |
| `adev cache clear --json` | `{"success": true}` |
| `adev screenshot --json --output <path>` | `{"success": true, "file": "..."}` |
| `adev record --json --output <path>` | `{"success": true, "file": "..."}` (runs until Ctrl+C) |

## Notes for agents

- `deep-clean` is destructive; it **requires `-y`** (or `--yes`) in `--json` /
  non-interactive mode, otherwise it errors instead of prompting. It deletes the
  project's `.gradle` and every `build/` directory but leaves `~/.gradle` intact.
- `uninstall` is destructive; it **requires `-y`** (or `--yes`) in `--json` /
  non-interactive mode.
- `--dry-run --json` emits `{"dry_run": true, ...}` for the resolved Gradle,
  ADB, file-system, or workflow action and does not execute side effects.
- For `install` / `test` / `clean` / `deep-clean`, Gradle's build output streams
  to the terminal; `--json` passes `-q` to keep stdout quiet, and the result
  object is the final line on stdout. Treat a non-zero exit as failure.
- `logs` streams logcat live and runs until interrupted. Use `--clear` before
  streaming, `--tag <TAG>` repeatedly for tag filters, `--level E` for a minimum
  level, and `--crashes` for common crash tags.
- If multiple devices are connected, device commands require `--device <serial>`
  in `--json` mode (otherwise they error rather than prompt). List them with
  `adev devices --json`.

## Example workflows

**Build, install, and launch on the default variant:**
```
adev info --json                 # confirm default_variant + application_id
adev install --json --device emulator-5554
adev launch  --json --device emulator-5554
```

**Build, install, and launch in one command:**
```
adev run --json --device emulator-5554
```

**Run a clean unit-test pass:**
```
adev test --fresh --json
```

**Run connected Android tests:**
```
adev connected-test --fresh --json --device emulator-5554
```

**Reset an app's state:**
```
adev clear-data --json --device emulator-5554
adev restart    --json --device emulator-5554
```
