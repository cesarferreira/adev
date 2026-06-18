<div align="center">
  <h1>adev</h1>

  <p><strong>Project-aware Android developer CLI — knows your repo's test, install, launch &amp; clean commands</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.74%2B-orange">
    <a href="https://crates.io/crates/adev"><img alt="crates.io" src="https://img.shields.io/crates/v/adev.svg"></a>
  </p>
</div>

---

Walk into any Android repo and run simple verbs without remembering this
project's variant names, Gradle task names, applicationId, or launcher activity:

```
adev test           instead of   ./gradlew testDevDebugUnitTest
adev install        instead of   ./gradlew installDevDebug
adev launch         instead of   adb shell am start -n com.foo/.MainActivity
adev deep-clean     instead of   ./gradlew --stop && rm -rf .gradle && find . -name build -delete
```

`adev` discovers your project structure automatically (modules, build variants,
`applicationId`, launch activity) and runs the correct command.

> Built on [`androkit`](https://github.com/cesarferreira/androkit), the shared
> Android toolkit it co-develops with [`dab`](https://github.com/cesarferreira/dab).

## Commands

| Command | What it does |
|---|---|
| `adev info` | Show modules, variants, applicationId, launch activity, and resolved tasks. |
| `adev install [variant]` | `./gradlew install<Variant>` (defaults to the resolved variant). |
| `adev launch` | Start the discovered launcher activity. |
| `adev test [--fresh]` | `./gradlew test<Variant>UnitTest` (`--fresh` adds `--rerun-tasks --no-build-cache`). |
| `adev logs` | Stream logcat filtered to this app. |
| `adev clean` | `./gradlew clean`. |
| `adev deep-clean [-y]` | Stop daemons, delete `.gradle` and every `build/` dir (keeps `~/.gradle`). Prompts unless `-y`. |
| `adev stop` / `clear-data` / `restart` | App lifecycle on the device. |
| `adev devices` | List connected devices. |
| `adev screenshot` / `record` | Capture screen (thin ADB wrappers). |

Run `adev` with no command for an interactive picker.

## Smart defaults

- **Variant resolution:** `--variant`/positional → project default (`devDebug` → `debug` → first available).
- **Task resolution:** AGP conventions — `install<Variant>`, `test<Variant>UnitTest`, `assemble<Variant>`.
- **Device resolution:** `--device` → the sole connected device → interactive picker.
- **Project discovery is cached** and invalidated when your build files change, so the inner loop stays fast.

## AI agent support

Every command accepts `--json` (data to stdout, `{"error": "..."}` to stderr,
exit code 0/1) and `--device` / `--variant` / `--module`, so agents never hit an
interactive prompt. See [`SKILL.md`](SKILL.md), installable with
`scripts/install-skill.sh`.

## Install

Requires [Rust](https://rustup.rs) **1.74+** and `~/.cargo/bin` on your `PATH`.

```bash
cargo install adev
```

<details>
<summary><strong>Build from source</strong> — for development or unreleased changes</summary>

```bash
git clone https://github.com/cesarferreira/adev.git
cd adev
cargo install --path . --locked   # expects ../androkit alongside
```

</details>

## Requirements

- Rust 1.74+
- `adb` on `PATH` (Android SDK platform-tools)
- A Gradle-based Android project with a `gradlew` wrapper

## Development

```bash
make              # check + build + test
make check        # cargo check + clippy
make lint         # fmt check + clippy
make test
make release LEVEL=patch   # requires cargo-release
```

## License

MIT
