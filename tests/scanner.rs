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
    assert_eq!(commerce.kind(), "Full-stack");
    assert!(commerce.stack.contains("React") && commerce.stack.contains("Spring Boot"));
    assert_eq!(commerce.components.len(), 2);
    let desktop = &report.projects[1];
    assert_eq!(desktop.kind(), "Desktop app");
    assert!(desktop.stack.contains("Tauri"));
    let flask = &report.projects[2];
    assert_eq!(flask.kind(), "Backend");
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
    assert_eq!(report.projects[0].kind(), "Full-stack");
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
    assert_eq!(first.projects[0].kind(), "Frontend");
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
    assert_eq!(portal.kind(), "Full-stack");
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
    assert_eq!(backend.kind(), "Backend");
    assert_eq!(frontend.kind(), "Frontend");
    assert!(frontend.repository.is_none());
    Ok(())
}

#[test]
fn distinguishes_gradle_tooling_back_ends_and_android_projects() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(
        root,
        "gradle/build.gradle",
        "plugins { id 'java' }\next.androidPluginId = 'com.android.application'\n",
    )?;
    put(
        root,
        "gradle/build.gradle.kts",
        "plugins {\n    id(\"com.android.application\") version \"8.7.3\" apply false\n}\n",
    )?;
    put(
        root,
        "spring/build.gradle.kts",
        "plugins { id(\"org.springframework.boot\") }\n",
    )?;
    put(
        root,
        "native/settings.gradle.kts",
        "rootProject.name = \"native\"\ninclude(\":app\")\n",
    )?;
    put(
        root,
        "native/app/build.gradle.kts",
        "plugins { alias(libs.plugins.android.application) }\n",
    )?;
    put(
        root,
        "native/app/src/main/AndroidManifest.xml",
        r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application /></manifest>"#,
    )?;
    put(
        root,
        "gradle-android/build.gradle",
        "apply plugin: 'com.android.library'\n",
    )?;

    let result = report(root)?;
    assert_eq!(result.projects.len(), 4);
    let find = |name: &str| result.projects.iter().find(|p| p.name() == name).unwrap();
    let tooling = find("gradle");
    assert_eq!(tooling.kind(), "Gradle · both DSLs");
    assert!(tooling.is_gradle() && !tooling.is_mobile());
    assert!(!tooling.is_android());
    let spring = find("spring");
    assert_eq!(spring.kind(), "Backend");
    assert!(spring.is_gradle() && !spring.is_mobile());
    let native = find("native");
    assert_eq!(native.kind(), "Android");
    assert!(native.is_gradle() && native.is_android() && native.is_mobile());
    assert!(!native.is_ios());
    assert!(
        native
            .components
            .iter()
            .any(|component| component.path.ends_with("native/app"))
    );
    assert_eq!(native.components.len(), 2);
    assert_eq!(find("gradle-android").kind(), "Android");
    Ok(())
}

#[test]
fn distinguishes_ios_from_macos_xcode_projects_and_swift_libraries() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(
        root,
        "phone/Phone.xcodeproj/project.pbxproj",
        "// !$*UTF8*$!\n{\n    buildSettings = {\n        SUPPORTED_PLATFORMS = \"iphoneos iphonesimulator\";\n    };\n}\n",
    )?;
    put(
        root,
        "desktop/Desktop.xcodeproj/project.pbxproj",
        "{ buildSettings = { SDKROOT = macosx; }; } // iphoneos was not used here\n",
    )?;
    put(
        root,
        "swift-library/Package.swift",
        "import PackageDescription\nlet package = Package(name: \"Utility\", platforms: [.iOS(.v15)])\n",
    )?;
    put(root, "not-xcode/project.pbxproj", "SDKROOT = iphoneos;\n")?;

    let result = report(root)?;
    assert_eq!(result.projects.len(), 3);
    let find = |name: &str| result.projects.iter().find(|p| p.name() == name).unwrap();
    let ios = find("phone");
    assert_eq!(ios.kind(), "iOS");
    assert!(ios.is_ios() && ios.is_mobile());
    assert!(!ios.is_gradle() && !ios.is_android());
    assert!(ios.stack.contains("Xcode"));
    assert_eq!(ios.components[0].path, ios.path);
    assert!(find("desktop").stack.contains("Xcode"));
    assert!(!find("desktop").is_mobile());
    assert!(!find("swift-library").is_mobile());
    Ok(())
}

#[test]
fn groups_flutter_platforms_and_keeps_expo_without_native_files_generic() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(
        root,
        "travel/pubspec.yaml",
        "name: travel\ndependencies:\n  flutter:\n    sdk: flutter\n",
    )?;
    put(root, "travel/android/settings.gradle", "include ':app'\n")?;
    put(
        root,
        "travel/android/app/build.gradle",
        "plugins { id 'com.android.application' }\n",
    )?;
    put(
        root,
        "travel/ios/Runner.xcodeproj/project.pbxproj",
        "{ buildSettings = { SDKROOT = iphoneos; }; }\n",
    )?;
    put(
        root,
        "prototype/package.json",
        r#"{"name":"prototype","dependencies":{"expo":"53","react":"19"}}"#,
    )?;

    let result = report(root)?;
    let [prototype, travel] = result.projects.as_slice() else {
        anyhow::bail!("expected two projects, found {}", result.projects.len());
    };
    assert_eq!(travel.name(), "travel");
    assert_eq!(travel.kind(), "Android + iOS");
    assert!(travel.is_gradle() && travel.is_android() && travel.is_ios() && travel.is_mobile());
    assert_eq!(travel.components.len(), 4);
    assert_eq!(prototype.name(), "prototype");
    assert_eq!(prototype.kind(), "Cross-platform");
    assert!(prototype.stack.contains("Expo"));
    assert!(!prototype.is_android() && !prototype.is_ios());
    Ok(())
}

#[test]
fn native_manifest_owners_cannot_escape_the_selected_scan_root() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    put(root, "app/src/main/AndroidManifest.xml", "<manifest />")?;
    put(
        root,
        "app/Phone.xcodeproj/project.pbxproj",
        "{ buildSettings = { SDKROOT = iphoneos; }; }\n",
    )?;
    for (relative, expected) in [("app/src/main", "Android"), ("app/Phone.xcodeproj", "iOS")] {
        let report = report(&root.join(relative))?;
        let [project] = report.projects.as_slice() else {
            anyhow::bail!("expected one project for {relative}");
        };
        assert_eq!(project.path, report.root);
        assert_eq!(project.kind(), expected);
    }
    Ok(())
}
