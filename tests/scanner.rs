use std::{fs, path::Path};

use anyhow::{Context as _, Result};
use project_analyser::scanner::{ScanControl, ScanReport, scan};

fn put(root: &Path, path: &str, contents: &str) -> Result<()> {
    let path = root.join(path);
    fs::create_dir_all(path.parent().context("fixture path has no parent")?)?;
    fs::write(path, contents)?;
    Ok(())
}

fn report(root: &Path) -> Result<ScanReport> {
    scan(root, &ScanControl::default())?.context("scan unexpectedly cancelled")
}

#[test]
fn detects_full_stack_conventions_and_standalone_apps() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(
        root,
        "commerce/frontend/package.json",
        r#"{"dependencies":{"react":"19"},"devDependencies":{"vite":"6"}}"#,
    )?;
    put(
        root,
        "commerce/backend/pom.xml",
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0"><parent><groupId>org.springframework.boot</groupId></parent></project>"#,
    )?;
    put(
        root,
        "python-demo/requirements.txt",
        "# docs\nrequests>=2\nFlask[async]==3.1\n",
    )?;
    put(
        root,
        "python-demo/src/app.py",
        "from flask import Flask\napp = Flask(__name__)\n",
    )?;
    put(
        root,
        "quick-api/app.py",
        "from fastapi import FastAPI\napp = FastAPI()\n",
    )?;
    put(
        root,
        "desktop/package.json",
        r#"{"dependencies":{"react":"19"}}"#,
    )?;
    put(
        root,
        "desktop/src-tauri/Cargo.toml",
        "[package]\nname='demo'\n[dependencies]\ntauri='2'\n",
    )?;
    let report = report(root)?;
    assert_eq!(report.projects.len(), 4);
    let commerce = &report.projects[0];
    assert_eq!(commerce.name(), "commerce");
    assert_eq!(commerce.kind(), "Full stack");
    assert!(commerce.stack.contains("React") && commerce.stack.contains("Spring Boot"));
    assert_eq!(commerce.components.len(), 2);
    let desktop = &report.projects[1];
    assert_eq!(desktop.kind(), "Desktop");
    assert!(desktop.stack.contains("Tauri"));
    let flask = &report.projects[2];
    assert_eq!(flask.kind(), "Back end");
    assert!(flask.stack.contains("Flask"));
    assert_eq!(flask.components.len(), 2);
    assert!(report.projects[3].stack.contains("FastAPI"));
    Ok(())
}

#[test]
fn groups_workspace_globs_and_respects_nested_repository_boundaries() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(
        root,
        "suite/pnpm-workspace.yaml",
        "packages:\n  - 'packages/*'\n  - '!packages/reference'\n",
    )?;
    put(
        root,
        "suite/packages/frontend/package.json",
        r#"{"dependencies":{"vue":"3"}}"#,
    )?;
    put(
        root,
        "suite/packages/backend/pyproject.toml",
        "[project]\nname='support'\ndependencies=['django>=5']\n",
    )?;
    put(
        root,
        "suite/packages/reference/package.json",
        r#"{"dependencies":{"svelte":"5"}}"#,
    )?;
    put(
        root,
        "suite/packages/external/package.json",
        r#"{"dependencies":{"electron":"32"}}"#,
    )?;
    put(
        root,
        "suite/packages/external/.git/config",
        "[remote \"origin\"]\nurl = git@github.com:sample/desktop.git\n",
    )?;
    put(
        root,
        "suite/packages/external/.git/refs/remotes/origin/main",
        &"f".repeat(40),
    )?;
    let report = report(root)?;
    assert_eq!(report.projects.len(), 3);
    assert_eq!(report.projects[0].kind(), "Full stack");
    assert_eq!(report.projects[0].components.len(), 3);
    let nested = &report.projects[1];
    assert_eq!(nested.name(), "external");
    let remotes = &nested
        .repository
        .as_ref()
        .context("missing repository")?
        .remotes;
    assert_eq!(remotes[0].location.slug(), "sample/desktop");
    assert!(remotes[0].has_tracking_refs);
    assert_eq!(report.projects[2].name(), "reference");
    Ok(())
}

#[test]
fn ignores_generated_files_and_does_not_modify_the_workspace() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(root, ".gitignore", "scratch/\n")?;
    put(root, ".git/config", "[core]\nrepositoryformatversion = 0\n")?;
    for directory in [
        "scratch",
        "node_modules/dependency",
        "target",
        ".venv/Lib",
        ".next",
    ] {
        put(
            root,
            &format!("{directory}/package.json"),
            r#"{"dependencies":{"express":"5"}}"#,
        )?;
    }
    put(
        root,
        "app/package.json",
        r#"{"dependencies":{"react":"19"}}"#,
    )?;
    put(root, "app/public/index.html", "<!doctype html>")?;
    let before = files(root)?;
    let first = report(root)?;
    let second = report(root)?;
    assert_eq!(first.projects, second.projects);
    assert_eq!(first.projects.len(), 1);
    assert_eq!(first.projects[0].path, dunce::canonicalize(root)?);
    assert_eq!(first.projects[0].components.len(), 1);
    assert_eq!(first.projects[0].kind(), "Front end");
    assert_eq!(before, files(root)?);
    Ok(())
}

fn files(root: &Path) -> Result<Vec<(std::path::PathBuf, Vec<u8>)>> {
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .build()
    {
        let entry = entry?;
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            out.push((entry.path().to_path_buf(), fs::read(entry.path())?));
        }
    }
    out.sort();
    Ok(out)
}

#[test]
fn handles_missing_root_oversized_manifests_and_cancellation() -> Result<()> {
    let temp = tempfile::tempdir()?;
    assert!(scan(&temp.path().join("missing"), &ScanControl::default()).is_err());
    put(temp.path(), "bad/package.json", &" ".repeat(300_000))?;
    let result = report(temp.path())?;
    assert!(result.projects.is_empty());
    assert!(result.skipped_entries > 0);
    let cancelled = ScanControl::default();
    cancelled.cancel();
    assert!(scan(temp.path(), &cancelled)?.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn never_descends_into_linked_folders() -> Result<()> {
    let outside = tempfile::tempdir()?;
    put(
        outside.path(),
        "package.json",
        r#"{"dependencies":{"react":"19"}}"#,
    )?;
    let temp = tempfile::tempdir()?;
    std::os::unix::fs::symlink(outside.path(), temp.path().join("linked"))?;
    std::os::unix::fs::symlink(
        outside.path().join("package.json"),
        temp.path().join("package.json"),
    )?;
    assert!(report(temp.path())?.projects.is_empty());
    Ok(())
}

#[test]
#[ignore = "manual local scan timing; not a CI time budget"]
fn measures_many_project_folders() -> Result<()> {
    let temp = tempfile::tempdir()?;
    for i in 0..500 {
        put(
            temp.path(),
            &format!("collection/app-{i}/package.json"),
            r#"{"dependencies":{"react":"19"}}"#,
        )?;
        put(
            temp.path(),
            &format!("collection/app-{i}/src/main.tsx"),
            "import { createRoot } from 'react-dom/client';\n",
        )?;
        put(
            temp.path(),
            &format!("collection/app-{i}/node_modules/ignored/package.json"),
            "{}",
        )?;
    }
    let result = report(temp.path())?;
    assert_eq!(result.projects.len(), 500);
    println!(
        "{} projects / {} visited directories / {} ms (scan only)",
        result.projects.len(),
        result.visited_directories,
        result.elapsed_ms
    );
    Ok(())
}

#[test]
fn groups_conventional_layouts_using_source_under_component_manifests() -> Result<()> {
    let temp = tempfile::tempdir()?;
    put(
        temp.path(),
        "portal/frontend/package.json",
        r#"{"name":"portal-ui"}"#,
    )?;
    put(
        temp.path(),
        "portal/frontend/src/main.tsx",
        "import { createRoot } from 'react-dom/client';\n",
    )?;
    put(
        temp.path(),
        "portal/backend/requirements.txt",
        "requests>=2\n",
    )?;
    put(
        temp.path(),
        "portal/backend/src/app.py",
        "from flask import Flask\n",
    )?;
    let result = report(temp.path())?;
    let [portal] = result.projects.as_slice() else {
        anyhow::bail!("expected one project")
    };
    assert_eq!(portal.name(), "portal");
    assert_eq!(portal.kind(), "Full stack");
    assert!(portal.stack.contains("React") && portal.stack.contains("Flask"));
    assert_eq!(portal.components.len(), 4);
    Ok(())
}

#[test]
fn keeps_independent_git_projects_out_of_conventional_layouts() -> Result<()> {
    let temp = tempfile::tempdir()?;
    put(
        temp.path(),
        "portal/front-end/package.json",
        r#"{"dependencies":{"react":"19"}}"#,
    )?;
    put(
        temp.path(),
        "portal/back-end/app.py",
        "from flask import Flask\n",
    )?;
    put(
        temp.path(),
        "portal/back-end/.git/config",
        "[remote \"origin\"]\nurl = git@github.com:sample/flask.git\n",
    )?;
    let result = report(temp.path())?;
    let [backend, frontend] = result.projects.as_slice() else {
        anyhow::bail!("expected two projects")
    };
    assert_eq!(backend.name(), "back-end");
    assert_eq!(frontend.name(), "front-end");
    assert_eq!(backend.kind(), "Back end");
    assert_eq!(frontend.kind(), "Front end");
    assert!(frontend.repository.is_none());
    Ok(())
}
