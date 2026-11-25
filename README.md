# Project Analyser

A read-only desktop dashboard for finding software projects in a folder. Built in Rust with [GPUI](https://gpui.rs/) and Longbridge's [GPUI Kit](https://github.com/longbridge/gpui-kit) (`gpui-component`).

## Build and run

Use a current stable Rust toolchain and the checked-in `Cargo.lock`. The first build compiles GPUI and is separate from application startup.

### Windows (x86-64 MSVC)

Install Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK. Use PowerShell from the repository root (an x64 Developer shell if needed):

```powershell
cargo run --locked
```

For an optimized build, GPUI also needs the SDK's `fxc.exe`. If it is not on `PATH`, point `GPUI_FXC_PATH` to the x64 compiler:

```powershell
$sdk = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
$fxc = Get-ChildItem "$sdk\*\x64\fxc.exe" -File |
    Sort-Object { [Version]$_.Directory.Parent.Name } -Descending |
    Select-Object -First 1
if (!$fxc) { throw 'Install a Windows SDK with the x64 fxc.exe compiler.' }
$env:GPUI_FXC_PATH = $fxc.FullName
cargo build --release --locked
Start-Process .\target\release\project-analyser.exe
```

### Arch Linux (x86-64)

```bash
sudo pacman -Syu --needed base-devel rust clang openssl pkgconf fontconfig \
  libx11 libxcb libxkbcommon libxkbcommon-x11 wayland vulkan-icd-loader
cargo build --release --locked
./target/release/project-analyser
```

If you already use `rustup`, use its stable toolchain instead of installing the `rust` package. Run the GUI under X11 or Wayland with a working Vulkan driver and installed fonts. The folder picker needs `xdg-desktop-portal` and a FileChooser-capable backend for your desktop. The release archive targets Arch Linux; it is not a portable binary for arbitrary Linux distributions.

### macOS

Install Xcode and select its developer directory. Check for the Metal compiler; download the Metal Toolchain via Xcode if the check fails:

```bash
xcrun --sdk macosx metal --version
# If the compiler is missing on a recent Xcode installation:
# xcodebuild -downloadComponent MetalToolchain
cargo build --release --locked
./target/release/project-analyser
```

The workflow builds an Apple silicon command-line archive from which you can launch the GUI binary. It is not a signed, notarized, or bundled `.app`.

## Use the dashboard

1. Click **Choose folder** and select a workspace or a parent directory. The selected path stays at the top; use **Copy root** if the path is truncated.
2. Search by project name, path, stack, Git repository, or saved description and tags. Select a result to see its classification, Git information, and the folders that contributed evidence.
3. Under **Notes & status**, choose **Edit**, enter a description, select one or more status tags, and click **Save**. Available tags: Planned, In progress, Developing, On hold, Completed, Archived, and Cancelled.
4. Use **Add favorite** in the details pane to star a project. **Favorites only** narrows the current workspace's inventory and combines with search. Favorites persist across launches.
5. Use **Refresh** to rescan or **Cancel scan** to stop. The dashboard shows skeletons while loading its first result and keeps its last result visible during a same-workspace refresh.

Click **Statistics** to see project counts by type, the six most common technologies, local Git and remote coverage, and the last successful scan's duration and folder counts. These figures cover the whole workspace, independent of search and the favorites filter. A technology counts once per project. Choose **Project details** or select a project to return to the details pane.

## What is detected?

The scanner recognizes project manifests, framework configuration, and selected source entry points. It supports, among others, React/Vue/Angular, Node back ends, Python/Flask/Django/FastAPI, Rust/Tauri, Spring Boot, and .NET. Grouping uses Git boundaries, workspace declarations, or conventional sibling directories. For example, a React `frontend` next to a Spring Boot `backend` can appear as **Full stack** with both stack labels and component paths. Classification is heuristic.

Git metadata comes from local configuration and refs. The dashboard shows the remote host, repository, and owner or organization, including a distinct push destination when present. **Tracking refs do not verify that your commits were pushed.** The app does not contact Git hosts.

## Local data and interruption recovery

**The scanner does not modify, execute, or upload scanned projects.** Descriptions, status tags, favorites, the last folder, and the latest successful inventory are saved to the app's `dashboard.json`. On Windows, the usual path is `%LOCALAPPDATA%\Workspace Tools\Project Analyser\data\dashboard.json`; **Copy data file path** gives the exact path. Close the app before editing or backing up the JSON. Notes and favorites are keyed by project path, so moving or renaming projects does not carry them over automatically.

If a terminal closure also stops a terminal-launched app, reopen the binary. The dashboard loads the last successfully saved workspace and matching snapshot and starts a fresh background scan. A forced stop during saving leaves the previous committed file intact. Unsaved edits and interrupted scans do not resume from their in-memory state. On Windows, launching the built binary from Explorer or with `Start-Process` avoids tying it to a foreground `cargo run`.

## Development and releases

```text
cargo fmt --all -- --check
cargo test --locked
cargo test --locked --no-default-features
cargo clippy --locked --all-targets -- -D warnings
```

To measure the scanner alone on a local 500-project fixture, run `cargo test --locked --no-default-features measures_many_project_folders -- --ignored --nocapture`. To measure the time to GPUI's first frame callback, set `PROJECT_ANALYSER_LOG_STARTUP=1` in the environment and launch an already built binary; compilation is not included. Real scan time varies with the workspace and filesystem.

On Windows, run `./scripts/validate-windows.ps1 -Detach` to run the checks and release build independently of the terminal. Each run writes `pid.txt`, `stdout.log`, `stderr.log`, and a final `exit-code.txt` under `target/validation/windows-*`. Reopen the terminal and check the reported directory rather than starting a duplicate run. Exit code `0` means all checks passed. If the file is absent, check the PID and logs before starting another run.

[Build and release](.github/workflows/build-release.yml) runs on pull requests, pushes to `main` or `master`, and manual dispatch. It uploads Windows x86-64, Arch Linux x86-64, and macOS Apple silicon archives. When a `v*` tag is pushed to a GitHub repository with Actions enabled, the workflow waits for all three platform jobs, then publishes the three archives and `SHA256SUMS`. Re-running a failed publish can resume the draft without overwriting an already published release.
