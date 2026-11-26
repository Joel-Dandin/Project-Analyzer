use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use super::{
    model::{ANDROID, GRADLE, GRADLE_KOTLIN, IOS, Project, Role},
    rules::Evidence,
};

pub(super) fn projects(
    evidence: BTreeMap<PathBuf, Evidence>,
    git_roots: &BTreeSet<PathBuf>,
) -> Vec<Project> {
    let mut groups = BTreeMap::<PathBuf, Project>::new();
    let roots = project_roots(&evidence, git_roots);
    // BTreeMap::iter and BTreeMap::into_iter both traverse in key order.
    for ((path, evidence), root) in evidence.into_iter().zip(roots) {
        let project = groups.entry(root.clone()).or_insert_with(|| Project {
            path: root,
            roles: BTreeSet::new(),
            stack: BTreeSet::new(),
            components: Vec::new(),
            repository: None,
        });
        let component = evidence.into_component(path);
        project.roles.extend(&component.roles);
        project.stack.extend(component.stack.iter().cloned());
        project.components.push(component);
    }
    groups.into_values().collect()
}

fn project_roots(
    evidence: &BTreeMap<PathBuf, Evidence>,
    git_roots: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut layout_signals = BTreeMap::<&Path, LayoutSignals>::new();
    let anchors: Vec<_> = evidence
        .iter()
        .map(|(path, entry)| {
            if let Some(repo) = path
                .ancestors()
                .find(|ancestor| git_roots.contains(*ancestor))
            {
                // Checked-in projects cannot contribute roles to a non-Git sibling layout.
                (repo, true)
            } else {
                let anchor = source_owner(path, entry, evidence);
                layout_signals.entry(anchor).or_default().add(entry);
                (anchor, false)
            }
        })
        .collect();
    let workspaces: HashMap<&Path, Workspace> = evidence
        .iter()
        .filter_map(|(path, entry)| {
            Workspace::new(&entry.includes, &entry.excludes)
                .map(|workspace| (path.as_path(), workspace))
        })
        .collect();
    let conventional = conventional_layouts(&layout_signals);
    anchors
        .into_iter()
        .map(|(mut root, in_git)| {
            if !in_git {
                while let Some(parent) = owner(root, &workspaces, &conventional) {
                    root = parent;
                }
            }
            root.to_path_buf()
        })
        .collect()
}

fn source_owner<'a>(
    path: &'a Path,
    evidence: &Evidence,
    all: &BTreeMap<PathBuf, Evidence>,
) -> &'a Path {
    if !evidence.declared
        && let Some(parent) = path
            .ancestors()
            .skip(1)
            .find(|parent| all.get(*parent).is_some_and(|entry| entry.declared))
        && let Ok(relative) = path.strip_prefix(parent)
        && let Some(first) = relative.iter().next().and_then(|part| part.to_str())
        && (matches!(
            first,
            "src" | "app" | "lib" | "cmd" | "internal" | "pkg" | "source"
        ) || parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.replace('-', "_") == first))
    {
        return parent;
    }
    path
}

fn owner<'a>(
    path: &'a Path,
    workspaces: &HashMap<&Path, Workspace>,
    conventional: &BTreeSet<&Path>,
) -> Option<&'a Path> {
    path.ancestors()
        .skip(1)
        .find(|parent| {
            workspaces.get(*parent).is_some_and(|workspace| {
                path.strip_prefix(parent)
                    .is_ok_and(|relative| workspace.contains(relative))
            })
        })
        .or_else(|| conventional.contains(path).then(|| path.parent()).flatten())
}

#[derive(Default)]
struct LayoutSignals {
    frontend: bool,
    backend: bool,
    tauri: bool,
    node: bool,
    gradle: bool,
    android: bool,
    ios: bool,
    mobile_container: bool,
}

impl LayoutSignals {
    fn add(&mut self, entry: &Evidence) {
        self.frontend |= entry.roles.contains(&Role::Frontend);
        self.backend |= entry.roles.contains(&Role::Backend);
        self.tauri |= entry.stack.contains("Tauri");
        self.node |= entry.stack.contains("Node.js");
        self.gradle |= entry.stack.contains(GRADLE) || entry.stack.contains(GRADLE_KOTLIN);
        self.android |= entry.stack.contains(ANDROID);
        self.ios |= entry.stack.contains(IOS);
        self.mobile_container |= ["Flutter", "React Native", "Expo"]
            .into_iter()
            .any(|framework| entry.stack.contains(framework));
    }
}

fn conventional_layouts<'a>(all: &BTreeMap<&'a Path, LayoutSignals>) -> BTreeSet<&'a Path> {
    let mut siblings = BTreeMap::<&Path, Pair>::new();
    let mut members = BTreeSet::new();
    for (&path, entry) in all {
        let Some(parent) = path.parent() else {
            continue;
        };
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // A cross-platform app's Android folder may only have workspace settings;
        // evidence from its app module is assigned to it by the workspace rule.
        if ((name == "android" && (entry.android || entry.gradle)) || (name == "ios" && entry.ios))
            && all
                .get(parent)
                .is_some_and(|parent| parent.mobile_container)
        {
            members.insert(path);
        }
        match name {
            "frontend" | "front-end" | "web" | "client" | "ui" if entry.frontend => {
                siblings.entry(parent).or_default().front.push(path);
            }
            "backend" | "back-end" | "server" | "api" if entry.backend => {
                siblings.entry(parent).or_default().back.push(path);
            }
            "src-tauri"
                if entry.tauri
                    && all
                        .get(parent)
                        .is_some_and(|parent| parent.frontend || parent.node) =>
            {
                members.insert(path);
            }
            _ => {}
        }
    }
    for pair in siblings.into_values() {
        if !pair.front.is_empty() && !pair.back.is_empty() {
            members.extend(pair.front.into_iter().chain(pair.back));
        }
    }
    members
}

#[derive(Default)]
struct Pair<'a> {
    front: Vec<&'a Path>,
    back: Vec<&'a Path>,
}

struct Workspace {
    included: GlobSet,
    excluded: GlobSet,
}

impl Workspace {
    fn new(includes: &[String], excludes: &[String]) -> Option<Self> {
        if includes.is_empty() {
            return None;
        }
        let included = patterns(
            includes
                .iter()
                .filter(|pattern| !pattern.starts_with('!'))
                .map(String::as_str),
        )?;
        if included.is_empty() {
            return None;
        }
        let excluded = patterns(
            excludes.iter().map(String::as_str).chain(
                includes
                    .iter()
                    .filter_map(|pattern| pattern.strip_prefix('!')),
            ),
        )?;
        Some(Self { included, excluded })
    }

    fn contains(&self, path: &Path) -> bool {
        self.included.is_match(path) && !self.excluded.is_match(path)
    }
}

fn patterns<'a>(values: impl Iterator<Item = &'a str>) -> Option<GlobSet> {
    let mut set = GlobSetBuilder::new();
    for value in values.take(256) {
        let normalized = value.trim().trim_start_matches("./").trim_end_matches('/');
        if normalized.is_empty()
            || normalized.len() > 512
            || normalized.split('/').any(|part| part == "..")
        {
            continue;
        }
        if let Ok(glob) = GlobBuilder::new(normalized)
            .literal_separator(true)
            .case_insensitive(cfg!(windows))
            .build()
        {
            set.add(glob);
        }
    }
    set.build().ok()
}
