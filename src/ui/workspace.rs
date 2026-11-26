use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};
use project_analyser::{
    scanner::{self, Project, ProjectKind, ScanControl, ScanReport},
    storage::{Library, Store, path_key},
};
use serde_json::{Value, json};

pub fn contains(roots: &[PathBuf], path: &Path) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

pub fn scan(
    folders: &[PathBuf],
    controls: &[ScanControl],
    progress: impl Fn(usize),
) -> Result<Option<ScanReport>> {
    let start = Instant::now();
    let mut result = ScanReport {
        root: folders.first().context("No scan folders")?.clone(),
        projects: vec![],
        visited_directories: 0,
        skipped_entries: 0,
        elapsed_ms: 0,
    };
    let mut projects = BTreeMap::new();
    for (i, (folder, control)) in folders.iter().zip(controls).enumerate() {
        // An overlapping selection is covered by its parent. This also handles older saved data.
        if !folders
            .iter()
            .any(|other| other != folder && folder.starts_with(other))
        {
            let Some(report) = scanner::scan(folder, control)? else {
                return Ok(None);
            };
            result.visited_directories += report.visited_directories;
            result.skipped_entries += report.skipped_entries;
            projects.extend(report.projects.into_iter().map(|p| (p.path.clone(), p)));
        }
        progress(i + 1);
    }
    if controls.iter().any(ScanControl::is_cancelled) {
        return Ok(None);
    }
    result.projects = projects.into_values().collect();
    result.elapsed_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
    Ok(Some(result))
}

fn kind(project: &Project) -> &'static str {
    match project.classification() {
        ProjectKind::Frontend => "frontend",
        ProjectKind::Backend => "backend",
        ProjectKind::FullStack => "full_stack",
        ProjectKind::Desktop => "desktop",
        ProjectKind::Android => "android",
        ProjectKind::Ios => "ios",
        ProjectKind::AndroidAndIos => "dual",
        ProjectKind::Mobile => "mobile",
        ProjectKind::Gradle => match (project.is_gradle_groovy(), project.is_gradle_kotlin()) {
            (true, true) => "gradle_mixed",
            (true, false) => "groovy",
            _ => "kotlin",
        },
        ProjectKind::Tooling => "tooling",
    }
}

fn project_view(project: &Project, library: &Library) -> Value {
    let path = &project.path;
    let annotation = library.annotation(path);
    let mobile = matches!(kind(project), "android" | "ios" | "dual" | "mobile");
    json!({
        "id": path_key(path), "name": project.name(), "path": path.to_string_lossy(),
        "kind": kind(project), "stack": project.stack, "mobile": mobile,
        "groovy": !mobile && project.is_gradle_groovy(),
        "kotlin": !mobile && project.is_gradle_kotlin(),
        "repository": project.repository, "starred": library.is_favorite(path),
        "summary": annotation.map(|a| a.description.as_str()).unwrap_or_default(),
        "notes": annotation.map(|a| a.notes.as_str()).unwrap_or_default(),
        "tags": annotation.map(|a| &a.custom_tags),
        "statuses": annotation.map(|a| &a.tags),
        "versions": annotation.map(|a| a.versions().collect::<Vec<_>>()),
        "todos": annotation.map(|a| a.todos()),
        "history": annotation.map(|a| a.status_history()),
        "components": project.components.iter().map(|part| json!({
            "path": part.path.to_string_lossy(), "stack": part.stack, "signals": part.signals,
        })).collect::<Vec<_>>(),
    })
}

pub fn state(
    library: &Library,
    store: Option<&Store>,
    loading: bool,
    progress: Option<(usize, bool)>,
    notice: Option<&str>,
    storage_error: Option<&str>,
) -> Value {
    let snapshot = library.snapshot();
    let folders = library.folders();
    let projects = snapshot
        .iter()
        .flat_map(|report| &report.projects)
        .filter(|p| contains(folders, &p.path))
        .map(|p| project_view(p, library))
        .collect::<Vec<_>>();
    let history = library
        .scan_history()
        .iter()
        .map(|entry| {
            json!({
                "at": entry.at, "projects": entry.projects, "elapsed_ms": entry.elapsed_ms,
                "folders": entry.folders.len(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "projects": projects, "history": history, "loading": loading,
        "editable": store.is_some(), "data_path": store.map(|s| s.path().to_string_lossy()),
        "cached": snapshot.is_some() && !folders.is_empty(),
        "last_report": snapshot.as_ref().map(|report| json!({
            "visited": report.visited_directories, "skipped": report.skipped_entries, "elapsed_ms": report.elapsed_ms,
        })),
        "progress": progress.map(|(done, cancelled)| json!({"done": done, "total": folders.len(), "cancelled": cancelled})),
        "notice": notice, "storage_error": storage_error,
        "home": directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_string_lossy().into_owned()),
        "windows": cfg!(windows),
        "folders": folders.iter().map(|folder| json!({
            "id": path_key(folder), "path": folder.to_string_lossy(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_scan_folders_and_cancels_without_publishing_partial_results() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let roots: Vec<_> = ["frontend", "backend"]
            .map(|name| temp.path().join(name))
            .to_vec();
        for root in &roots {
            std::fs::create_dir(root)?;
        }
        std::fs::write(
            roots[0].join("package.json"),
            r#"{"dependencies":{"react":"19"}}"#,
        )?;
        std::fs::write(roots[1].join("build.gradle.kts"), "plugins { java }")?;
        let controls = [ScanControl::default(), ScanControl::default()];
        let report = scan(&roots, &controls, |_| {})?.unwrap();
        assert_eq!(report.projects.len(), 2);
        assert_eq!(kind(&report.projects[0]), "kotlin");
        assert_eq!(kind(&report.projects[1]), "frontend");
        let controls = [ScanControl::default(), ScanControl::default()];
        assert!(
            scan(&roots, &controls, |done| if done == 1 {
                controls[1].cancel()
            })?
            .is_none()
        );
        Ok(())
    }
}
