# Project Analyser

A read-only desktop dashboard for finding software projects in a folder. Built in Rust with [GPUI](https://gpui.rs/) and Longbridge's [GPUI Kit](https://github.com/longbridge/gpui-kit) (`gpui-component`).

## Run on Windows

Use Rust **1.90 or newer** with the MSVC toolchain, Visual Studio Build Tools with **Desktop development with C++**, and a Windows SDK. From the repository root:

```powershell
cargo run --locked
```

The first build compiles GPUI and is separate from application startup. To build once and launch the optimized app directly:

```powershell
cargo build --release --locked
.\target\release\project-analyser.exe
```

## Use the dashboard

1. Click **Choose folder** and select a workspace or a parent directory. The selected path stays at the top; use **Copy root** if the path is truncated.
2. Search by project name, path, stack, Git repository, or saved description and tags. Select a result to see its classification, Git information, and the folders that contributed evidence.
3. Under **Notes & status**, choose **Edit**, enter a description, select one or more status tags, and click **Save**. Available tags: Planned, In progress, Developing, On hold, Completed, Archived, and Cancelled.
4. Use **Refresh** to rescan or **Cancel scan** to stop. The app shows skeletons while loading its first result and keeps the existing inventory available during refresh. On reopening, it restores the last workspace and cached results while rescanning in the background.

## What is detected?

The scanner recognizes project manifests, framework configuration, and selected source entry points. It supports, among others, React/Vue/Angular, Node back ends, Python/Flask/Django/FastAPI, Rust/Tauri, Spring Boot, and .NET. Grouping uses Git boundaries, workspace declarations, or conventional sibling directories. For example, a React `frontend` next to a Spring Boot `backend` can appear as **Full stack** with both stack labels and both component paths. Classification is heuristic; see [.ai/DETECTION.md](.ai/DETECTION.md) for rule coverage.

The dashboard shows the Git remote host, repository, and owner or organization inferred from local configuration, including a distinct push destination when present. It differentiates a configured remote from one with locally recorded tracking refs. **Tracking refs do not verify that your commits were pushed.** The app does not contact Git hosts.

## Local data and privacy

**The scanner does not modify, execute, or upload scanned projects.** Descriptions, status tags, the last selected folder, and the latest successful inventory are saved together in the app's `dashboard.json`. On Windows, the usual path is `%LOCALAPPDATA%\Workspace Tools\Project Analyser\data\dashboard.json`; **Copy data file path** gives the exact path. Close the app before editing or backing up the JSON. Notes are keyed by project path, so moving or renaming a project does not automatically move its notes.

## Development

```powershell
cargo fmt --check
cargo test --locked
cargo test --no-default-features --locked
cargo clippy --all-targets --locked -- -D warnings
```

The scanner runs on bounded background workers and prunes common dependency, build, and cache directories. The 500-project fixture and startup measurement instructions are in [.ai/VALIDATION.md](.ai/VALIDATION.md). Real scan duration depends on filesystem speed and directory size.

Start with [.ai/README.md](.ai/README.md) for the implementation plan, architecture, detection rules, and outstanding work.
