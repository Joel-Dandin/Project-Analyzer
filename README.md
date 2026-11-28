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
3. **Browse.** The sidebar groups project types under **Web**, **Mobile**, **Build system**, and **Other**. Every project has one primary type; the sidebar, inventory, project details, and statistics use the same labels. Click a type again to clear it.
4. **Filter.** **+ Filter** searches name, path, stack, Git host/owner/repository, summary, notes, tags, versions, and status. Its platform and DSL filters can match projects with a different primary type. **☆ Starred** shows favorites only. **Search version label…** matches version labels. **Display** groups by type and toggles the details pane.
5. **Project details.** Use **Open folder** next to the path or **Open in browser** next to the Git repository. The details also contain status, stack, tags, versions, summary, notes, to-dos, and status history. The project's Markdown **Scratchpad** is separate from notes and to-dos. Pick **Write**, **Split**, or **Preview**, then click **Save scratchpad**.
6. **Idea board.** Open **Idea board → + New idea** for an independent project idea. Add a title, Markdown, tags, and a status, then click **Save idea**. Search by title, tags, or the opening snippet, or filter by status. Choose **Promoted** and select an existing scanned project to link the idea; this doesn't create any files or folders. **Ctrl+Enter** (or **⌘+Enter** on macOS) saves in either editor.
7. **Statistics.** Totals, active, starred, Git coverage, version-labelled, open to-dos, projects by type, top languages and frameworks, status, Git hosts, owners, scan activity, tags, versions, to-do completion, and recent status changes. These figures cover the whole workspace regardless of filters. **Export** writes them as JSON.

Classification is heuristic. Android and iOS use native platform evidence; Flutter, React Native, and Expo can appear as **Cross-platform** without that evidence. **Library / SDK**, **CLI tool**, **Data / ML**, and **DevOps / Infra** use signals such as declared package exports or commands, machine-learning dependencies, and Terraform or Helm manifests. Gradle is a build tool, not a mobile signal.

Git metadata comes from local configuration and refs only. **Tracking refs do not verify that your commits were pushed.** Scanning never contacts a Git host. The explicit **Open in browser** action opens the detected primary remote as an HTTPS address in your default browser, without Git credentials or query parameters.

## Local data and interruption recovery

**The scanner does not modify, execute, or upload scanned projects.** Summaries, notes, tags, version labels, status tags and history, to-dos, Markdown scratchpads, standalone ideas, favorites, scan folders, scan history, and the latest inventory are saved to the app's `dashboard.json` (on Windows, usually `%LOCALAPPDATA%\Workspace Tools\Project Analyser\data\dashboard.json`). Earlier versions' data is read and upgraded in place. Project metadata is keyed by project path, so moving or renaming projects does not carry it over.

Writes are atomic: a forced stop while saving leaves the previously committed file intact. If a terminal closure stops a terminal-launched app, reopen it: it loads the last saved state and starts a fresh scan. Unsaved Markdown drafts stay in memory when switching views or rescanning, but they and interrupted scans do not resume after the app exits. Markdown previews work offline: raw HTML is shown as text, and links and images are not loaded. On Windows, launching the built binary from Explorer or with `Start-Process` avoids tying it to a foreground `cargo run`.

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
