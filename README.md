# Project Analyser

A read-only desktop dashboard for finding software projects in one or more folders. The Rust scanner and storage code drive a native window (`tao`) that hosts the dashboard in the operating system's WebView (`wry`). The interface is the Linear-style design from `temp/Design 3`, embedded in the binary: it needs no network access and no Node tooling.

## Build and run

Use a current stable Rust toolchain (1.90 or newer) and the checked-in `Cargo.lock`.

### Windows (x86-64 MSVC)

Install Visual Studio Build Tools with **Desktop development with C++**. The WebView2 runtime ships with Windows 11 and current Windows 10 updates; install the Evergreen runtime from Microsoft if it is missing.

```powershell
cargo run --locked
# or an optimized build:
cargo build --release --locked
Start-Process .\target\release\project-analyser.exe
```

### Arch Linux (x86-64)

```bash
sudo pacman -Syu --needed base-devel rust pkgconf gtk3 webkit2gtk-4.1 xdg-desktop-portal
cargo build --release --locked
./target/release/project-analyser
```

If you already use `rustup`, use its stable toolchain instead of the `rust` package. The folder picker needs a desktop portal backend for your desktop. The release archive targets Arch Linux; it is not a portable binary for arbitrary distributions.

### macOS

Install the Xcode command line tools (`xcode-select --install`). WKWebView is part of the system.

```bash
cargo build --release --locked
./target/release/project-analyser
```

The workflow builds an Apple silicon command-line archive. It is not a signed, notarized, or bundled `.app`.

## Use the dashboard

1. **Scan folders.** Click **+ Choose folder** (or **Start scan** on first launch) and select one or more folders. A folder inside an already selected folder is covered by its parent. Remove a folder with **×** while no scan is running.
2. **Start scan / Rescan.** The header shows `Scanning · n of m folders`. Use **Cancel scan** to stop; a cancelled scan keeps the previous results. On launch the last saved results appear immediately ("Cached … · refreshing") while a fresh scan runs.
3. **Browse.** The sidebar lists **All projects**, **Starred**, **Statistics**, and the project types: Frontend, Backend, Full-stack, Android only, iOS only, Android + iOS, Gradle (Groovy DSL), Gradle (Kotlin DSL). Desktop, other mobile, mixed-DSL Gradle, and general/tooling groups appear when present. Click a type again to clear it.
4. **Filter.** **+ Filter** searches name, path, stack, Git host/owner/repository, summary, notes, tags, versions, and status, and offers **Android (any)**, **iOS (any)**, and **Gradle (any DSL)**. **☆ Starred** shows favorites only. **Search version label…** matches version labels. **Display** groups by type and toggles the details pane.
5. **Project details.** Star a project, change its status (multiple statuses are allowed), read-only Git host / owner / repository, detected stack, tags, version labels, summary, notes, to-dos, and a timestamped status history (UTC). Edits save as you make them.
6. **Statistics.** Totals, active, starred, Git coverage, version-labelled, open to-dos, projects by type, top languages and frameworks, status, Git hosts, owners, scan activity, tags, versions, to-do completion, and recent status changes. These figures cover the whole workspace regardless of filters. **Export** writes them as JSON.

Classification is heuristic. Gradle is a build tool, not a mobile indicator: Android detection uses an applied Android Gradle plugin or an `AndroidManifest.xml`; iOS detection uses iPhone/iPad build settings in an Xcode project. Flutter, React Native, and Expo projects may be listed as **Other mobile** until native platform evidence is found.

Git metadata comes from local configuration and refs only. **Tracking refs do not verify that your commits were pushed.** The app does not contact Git hosts.

## Local data and interruption recovery

**The scanner does not modify, execute, or upload scanned projects.** Summaries, notes, tags, version labels, status tags and history, to-dos, favorites, scan folders, scan history, and the latest inventory are saved to the app's `dashboard.json` (on Windows, usually `%LOCALAPPDATA%\Workspace Tools\Project Analyser\data\dashboard.json`). Earlier versions' data is read and upgraded in place. Project metadata is keyed by project path, so moving or renaming projects does not carry it over.

Writes are atomic: a forced stop while saving leaves the previously committed file intact. If a terminal closure stops a terminal-launched app, reopen it: it loads the last saved state and starts a fresh scan. Unsaved in-memory state and interrupted scans do not resume. On Windows, launching the built binary from Explorer or with `Start-Process` avoids tying it to a foreground `cargo run`.

The embedded page can only load its own assets, cannot open other URLs or windows, and runs without WebView storage (`incognito`). Only a typed set of commands reaches Rust.

## Development and releases

```text
cargo fmt --all -- --check
cargo test --locked
cargo test --locked --no-default-features
cargo clippy --locked --all-targets -- -D warnings
```

To measure the scanner alone on a local 500-project fixture, run `cargo test --locked --no-default-features measures_many_project_folders -- --ignored --nocapture`. To log the time until the page asks for data, set `PROJECT_ANALYSER_LOG_STARTUP=1` and launch an already built binary.

On Windows, run `./scripts/validate-windows.ps1 -Detach` to run the checks and release build independently of the terminal. Each run writes `pid.txt`, `stdout.log`, `stderr.log`, and a final `exit-code.txt` under `target/validation/windows-*`. Exit code `0` means all checks passed. If the file is absent, check the PID and logs before starting another run.

[Build and release](.github/workflows/build-release.yml) runs on pull requests, pushes to `main` or `master`, and manual dispatch. It uploads Windows x86-64, Arch Linux x86-64, and macOS Apple silicon archives, each with the bundled font licenses. When a `v*` tag is pushed to a GitHub repository with Actions enabled, the workflow waits for all three platform jobs, then publishes the three archives and `SHA256SUMS`. Re-running a failed publish can resume the draft without overwriting an already published release.

Embedded fonts: Inter and JetBrains Mono, both under the SIL Open Font License (`src/ui/web/*-OFL.txt`).
