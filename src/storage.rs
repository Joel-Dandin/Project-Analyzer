//! App-owned persistence. No annotation or scan report is written into a project.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File},
    io::{self, BufReader, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result, ensure};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::scanner::ScanReport;

const SCHEMA_VERSION: u32 = 3;
pub const VERSION_LIMIT: usize = 64;
pub const TODO_LIMIT: usize = 240;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Planned,
    InProgress,
    Developing,
    OnHold,
    Completed,
    Archived,
    Cancelled,
    InReview,
}

impl Tag {
    pub const ALL: [Self; 8] = [
        Self::Planned,
        Self::InProgress,
        Self::Developing,
        Self::OnHold,
        Self::Completed,
        Self::Archived,
        Self::Cancelled,
        Self::InReview,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Planned => "Planned",
            Self::InProgress => "In progress",
            Self::Developing => "Developing",
            Self::OnHold => "On hold",
            Self::Completed => "Completed",
            Self::Archived => "Archived",
            Self::Cancelled => "Cancelled",
            Self::InReview => "In review",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Annotation {
    pub description: String,
    pub tags: BTreeSet<Tag>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub custom_tags: BTreeSet<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    older_versions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    status_history: Vec<StatusChange>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    todos: Vec<Todo>,
}

impl Annotation {
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        (!self.version.is_empty())
            .then_some(self.version.as_str())
            .into_iter()
            .chain(self.older_versions.iter().map(String::as_str))
    }
    /// In order of saving, from oldest to newest.
    pub fn status_history(&self) -> &[StatusChange] {
        &self.status_history
    }

    pub fn todos(&self) -> &[Todo] {
        &self.todos
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusChange {
    pub at: DateTime<Utc>,
    pub previous: BTreeSet<Tag>,
    pub current: BTreeSet<Tag>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub completed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScanSummary {
    pub at: DateTime<Utc>,
    pub folders: Vec<PathBuf>,
    pub projects: usize,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Library {
    schema_version: u32,
    #[serde(default)]
    pub last_root: Option<PathBuf>,
    #[serde(default)]
    snapshot: Option<Arc<ScanReport>>,
    #[serde(default)]
    annotations: Arc<BTreeMap<String, Annotation>>,
    #[serde(default)]
    favorites: Arc<BTreeSet<String>>,
    // None means the dashboard predates multiple scan folders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    folders: Option<Vec<PathBuf>>,
    #[serde(default)]
    scan_history: Vec<ScanSummary>,
    #[serde(default)]
    pub scan_pending: bool,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_root: None,
            snapshot: None,
            annotations: Arc::new(BTreeMap::new()),
            favorites: Arc::new(BTreeSet::new()),
            folders: None,
            scan_history: Vec::new(),
            scan_pending: false,
        }
    }
}

impl Library {
    pub fn annotation(&self, project: &Path) -> Option<&Annotation> {
        self.annotations.get(&path_key(project))
    }

    /// Saves the editable fields without replacing the project's history or to-dos.
    /// Returns whether anything changed. Unchanged status sets are not logged.
    pub fn set_annotation(
        &mut self,
        project: &Path,
        description: String,
        tags: BTreeSet<Tag>,
        version: &str,
    ) -> bool {
        let key = path_key(project);
        let version: String = version.trim().chars().take(VERSION_LIMIT).collect();
        let unchanged = self.annotations.get(&key).map_or_else(
            || description.is_empty() && tags.is_empty() && version.is_empty(),
            |entry| {
                entry.description == description && entry.tags == tags && entry.version == version
            },
        );
        if unchanged {
            return false;
        }
        let annotations = Arc::make_mut(&mut self.annotations);
        let entry = annotations.entry(key.clone()).or_default();
        if entry.tags != tags {
            entry.status_history.push(StatusChange {
                at: Utc::now(),
                previous: std::mem::replace(&mut entry.tags, tags),
                current: entry.tags.clone(),
            });
        }
        entry.description = description;
        entry.version = version;
        if *entry == Annotation::default() {
            annotations.remove(&key);
        }
        true
    }

    pub fn set_notes(&mut self, project: &Path, notes: String) -> bool {
        self.edit_annotation(project, |entry| {
            if entry.notes == notes {
                return false;
            }
            entry.notes = notes;
            true
        })
    }

    pub fn set_custom_tag(&mut self, project: &Path, tag: &str, present: bool) -> bool {
        let tag: String = tag.trim().chars().take(64).collect();
        if tag.is_empty() {
            return false;
        }
        self.edit_annotation(project, |entry| {
            if present {
                entry.custom_tags.insert(tag)
            } else {
                entry.custom_tags.remove(&tag)
            }
        })
    }

    pub fn set_version_label(&mut self, project: &Path, version: &str, present: bool) -> bool {
        let version: String = version.trim().chars().take(VERSION_LIMIT).collect();
        if version.is_empty() {
            return false;
        }
        self.edit_annotation(project, |entry| {
            if present {
                if entry.versions().any(|label| label == version) {
                    return false;
                }
                let previous = std::mem::replace(&mut entry.version, version);
                if !previous.is_empty() {
                    entry.older_versions.insert(0, previous);
                }
                true
            } else if entry.version == version {
                entry.version = if entry.older_versions.is_empty() {
                    String::new()
                } else {
                    entry.older_versions.remove(0)
                };
                true
            } else {
                let len = entry.older_versions.len();
                entry.older_versions.retain(|label| *label != version);
                entry.older_versions.len() != len
            }
        })
    }

    fn edit_annotation(
        &mut self,
        project: &Path,
        edit: impl FnOnce(&mut Annotation) -> bool,
    ) -> bool {
        let key = path_key(project);
        let annotations = Arc::make_mut(&mut self.annotations);
        let entry = annotations.entry(key.clone()).or_default();
        let changed = edit(entry);
        if *entry == Annotation::default() {
            annotations.remove(&key);
        }
        changed
    }

    pub fn add_todo(&mut self, project: &Path, title: &str) -> bool {
        let title: String = title.trim().chars().take(TODO_LIMIT).collect();
        if title.is_empty() {
            return false;
        }
        let todos = &mut Arc::make_mut(&mut self.annotations)
            .entry(path_key(project))
            .or_default()
            .todos;
        let Some(id) = todos
            .iter()
            .map(|todo| todo.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            return false;
        };
        todos.push(Todo {
            id,
            title,
            completed: false,
        });
        true
    }

    pub fn set_todo_completed(&mut self, project: &Path, id: u64, completed: bool) -> bool {
        self.update_todo(project, id, |todos, index| {
            let todo = &mut todos[index];
            if todo.completed == completed {
                return false;
            }
            todo.completed = completed;
            true
        })
    }

    pub fn remove_todo(&mut self, project: &Path, id: u64) -> bool {
        self.update_todo(project, id, |todos, index| {
            todos.remove(index);
            true
        })
    }

    fn update_todo(
        &mut self,
        project: &Path,
        id: u64,
        update: impl FnOnce(&mut Vec<Todo>, usize) -> bool,
    ) -> bool {
        let key = path_key(project);
        let Some(index) = self
            .annotations
            .get(&key)
            .and_then(|entry| entry.todos.iter().position(|todo| todo.id == id))
        else {
            return false;
        };
        let annotations = Arc::make_mut(&mut self.annotations);
        let entry = annotations.get_mut(&key).expect("project has the to-do");
        let changed = update(&mut entry.todos, index);
        if *entry == Annotation::default() {
            annotations.remove(&key);
        }
        changed
    }

    pub fn is_favorite(&self, project: &Path) -> bool {
        self.favorites.contains(&path_key(project))
    }

    /// Favorites are independent of notes and the currently loaded workspace.
    /// Returns whether the stored preference changed.
    pub fn set_favorite(&mut self, project: &Path, favorite: bool) -> bool {
        let key = path_key(project);
        if self.favorites.contains(&key) == favorite {
            return false;
        }
        let favorites = Arc::make_mut(&mut self.favorites);
        if favorite {
            favorites.insert(key);
        } else {
            favorites.remove(&key);
        }
        true
    }

    pub fn snapshot(&self) -> Option<Arc<ScanReport>> {
        self.snapshot
            .as_ref()
            .filter(|report| self.last_root.as_ref() == Some(&report.root))
            .cloned()
    }

    pub fn folders(&self) -> &[PathBuf] {
        self.folders
            .as_deref()
            .unwrap_or_else(|| self.last_root.as_slice())
    }

    pub fn set_folders(&mut self, folders: Vec<PathBuf>) {
        self.folders = Some(folders);
    }

    pub fn scan_history(&self) -> &[ScanSummary] {
        &self.scan_history
    }

    pub fn record_workspace_scan(&mut self, report: Arc<ScanReport>) {
        self.scan_pending = false;
        if self.scan_history.len() >= 32 {
            self.scan_history.remove(0);
        }
        self.scan_history.push(ScanSummary {
            at: Utc::now(),
            folders: self.folders().to_vec(),
            projects: report.projects.len(),
            elapsed_ms: report.elapsed_ms,
        });
        self.record_scan(report);
    }

    pub fn record_scan(&mut self, report: Arc<ScanReport>) {
        self.last_root = Some(report.root.clone());
        self.snapshot = Some(report);
    }
}

pub fn path_key(path: &Path) -> String {
    if let Some(path) = path.to_str() {
        return path.to_owned();
    }
    // Scanned project paths are absolute. This prefix cannot collide with one.
    use fmt::Write as _;
    let raw = path.as_os_str().as_encoded_bytes();
    let mut key = String::with_capacity(8 + raw.len() * 2);
    key.push_str("@native:");
    for byte in raw {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn for_user() -> Result<Self> {
        let dirs = ProjectDirs::from("org", "Workspace Tools", "Project Analyser")
            .context("No user application data directory is available")?;
        Ok(Self {
            path: dirs.data_local_dir().join("dashboard.json"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Library> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Library::default()),
            Err(err) => return Err(err).context("Could not open dashboard data"),
        };
        let mut data: Library = serde_json::from_reader(BufReader::new(file))
            .context("Could not parse dashboard.json")?;
        ensure!(
            (1..=SCHEMA_VERSION).contains(&data.schema_version),
            "Dashboard data has unsupported schema version {}",
            data.schema_version
        );
        // Serde supplies defaults for newly added fields;
        // existing tags are not dated or assigned a made-up history entry.
        data.schema_version = SCHEMA_VERSION;
        Ok(data)
    }

    pub fn save(&self, data: &Library) -> Result<()> {
        self.stage(data)?
            .persist(&self.path)
            .context("Could not replace dashboard.json")?;
        Ok(())
    }

    fn stage(&self, data: &Library) -> Result<NamedTempFile> {
        let parent = self.path.parent().context("Missing app data directory")?;
        fs::create_dir_all(parent).context("Could not create the app data directory")?;
        let mut temp = NamedTempFile::new_in(parent).context("Could not stage dashboard data")?;
        // Buffer writes: pretty-printing a large report emits many small fragments.
        {
            let mut writer = io::BufWriter::new(&mut temp);
            serde_json::to_writer_pretty(&mut writer, data)
                .context("Could not encode dashboard data")?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temp.as_file().sync_all()?;
        Ok(temp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v2_and_persists_new_annotations_and_scan_history() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store {
            path: dir.path().join("dashboard.json"),
        };
        let root = dir.path().join("workspace");
        let project = root.join("app");
        fs::write(
            store.path(),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 2,
                "last_root": root,
                "annotations": { (path_key(&project)): { "description": "Summary", "version": "v1", "tags": ["developing"] } }
            }))?,
        )?;

        let mut data = store.load()?;
        assert_eq!(data.folders(), std::slice::from_ref(&root));
        assert!(data.set_notes(&project, "Separate notes".into()));
        assert!(data.set_custom_tag(&project, " internal ", true));
        assert!(!data.set_custom_tag(&project, "internal", true));
        assert!(data.set_version_label(&project, "v2", true));
        assert!(data.set_version_label(&project, "v3", true));
        assert!(!data.set_version_label(&project, "v1", true));
        assert!(data.set_version_label(&project, "v3", false));
        data.set_folders(vec![root.clone(), dir.path().join("another")]);
        let report = Arc::new(ScanReport {
            root,
            projects: vec![],
            visited_directories: 1,
            skipped_entries: 0,
            elapsed_ms: 12,
        });
        data.scan_pending = true;
        store.save(&data)?;
        let mut data = store.load()?;
        assert!(data.scan_pending);
        data.record_workspace_scan(report);
        store.save(&data)?;
        let data = store.load()?;
        let entry = data.annotation(&project).unwrap();
        assert_eq!(entry.description, "Summary");
        assert_eq!(entry.notes, "Separate notes");
        assert_eq!(entry.custom_tags, BTreeSet::from(["internal".into()]));
        assert_eq!(entry.versions().collect::<Vec<_>>(), ["v2", "v1"]);
        assert_eq!(entry.tags, BTreeSet::from([Tag::Developing]));
        assert!(!data.scan_pending);
        assert_eq!(data.scan_history()[0].folders, data.folders());
        assert_eq!(data.scan_history()[0].elapsed_ms, 12);
        assert!(!project.exists());
        Ok(())
    }

    #[test]
    fn round_trips_and_replaces_state() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store {
            path: temp.path().join("dashboard").join("dashboard.json"),
        };
        let root = temp.path().join("workspace");
        let project = root.join("sample");
        let mut data = store.load()?;
        data.last_root = Some(root.clone());
        let tags = BTreeSet::from([Tag::Developing, Tag::OnHold]);
        assert!(data.set_annotation(&project, "Demo API".into(), tags.clone(), " 1.2.0 "));
        assert!(!data.set_annotation(&project, "Demo API".into(), tags.clone(), "1.2.0"));
        assert!(data.add_todo(&project, "Write docs"));
        let task_id = data.annotation(&project).unwrap().todos()[0].id;
        assert!(data.set_todo_completed(&project, task_id, true));
        let annotation = data.annotation(&project).unwrap().clone();
        assert_eq!(annotation.version, "1.2.0");
        assert_eq!(annotation.status_history().len(), 1);
        assert!(annotation.status_history()[0].previous.is_empty());
        assert_eq!(annotation.status_history()[0].current, tags);
        let other_project = root.join("no-notes");
        assert!(data.set_favorite(&project, true));
        assert!(!data.set_favorite(&project, true));
        assert!(data.set_favorite(&other_project, true));
        let report = Arc::new(ScanReport {
            root: root.clone(),
            projects: vec![],
            skipped_entries: 0,
            visited_directories: 1,
            elapsed_ms: 2,
        });
        data.record_scan(report.clone());
        store.save(&data)?;
        let mut loaded = store.load()?;
        assert_eq!(loaded.last_root, Some(root));
        assert_eq!(loaded.annotation(&project), Some(&annotation));
        assert_eq!(loaded.snapshot(), Some(report));
        assert!(loaded.is_favorite(&project));
        assert!(loaded.is_favorite(&other_project));
        assert!(loaded.annotation(&other_project).is_none());
        loaded.last_root = Some(temp.path().join("different-workspace"));
        assert!(loaded.snapshot().is_none());
        assert!(loaded.set_favorite(&project, false));
        assert!(!loaded.set_favorite(&project, false));
        store.save(&loaded)?;
        let loaded = store.load()?;
        assert!(!loaded.is_favorite(&project));
        assert!(loaded.is_favorite(&other_project));
        assert_eq!(loaded.annotation(&project), Some(&annotation));
        let mut cleared = loaded;
        assert!(cleared.set_annotation(&project, String::new(), BTreeSet::new(), ""));
        store.save(&cleared)?;
        let cleared = store.load()?;
        let entry = cleared.annotation(&project).unwrap();
        assert!(entry.version.is_empty());
        assert!(entry.tags.is_empty());
        assert_eq!(entry.status_history().len(), 2);
        assert_eq!(entry.status_history()[1].previous, tags);
        assert!(entry.status_history()[1].current.is_empty());
        assert_eq!(entry.todos(), annotation.todos());
        assert!(cleared.is_favorite(&other_project));
        assert!(!project.exists() && !other_project.exists());
        Ok(())
    }

    #[test]
    fn loads_version_one_data_without_favorites() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store {
            path: temp.path().join("dashboard.json"),
        };
        fs::write(
            store.path(),
            r#"{"schema_version":1,"annotations":{"/project":{"description":"Existing note","tags":["planned"]}}}"#,
        )?;
        let mut data = store.load()?;
        let project = Path::new("/project");
        assert!(!data.is_favorite(project));
        let annotation = data.annotation(project).unwrap();
        assert_eq!(annotation.description, "Existing note");
        assert_eq!(annotation.tags, BTreeSet::from([Tag::Planned]));
        assert!(annotation.version.is_empty());
        assert!(annotation.status_history().is_empty());
        assert!(annotation.todos().is_empty());
        assert!(data.set_annotation(
            project,
            "Edited note".into(),
            BTreeSet::from([Tag::Planned]),
            "v1"
        ));
        assert!(
            data.annotation(project)
                .unwrap()
                .status_history()
                .is_empty()
        );
        assert!(data.set_annotation(
            project,
            "Edited note".into(),
            BTreeSet::from([Tag::Developing]),
            "v1"
        ));
        let event = &data.annotation(project).unwrap().status_history()[0];
        assert_eq!(event.previous, BTreeSet::from([Tag::Planned]));
        assert_eq!(event.current, BTreeSet::from([Tag::Developing]));
        store.save(&data)?;
        assert_eq!(store.load()?.schema_version, SCHEMA_VERSION);
        Ok(())
    }

    #[test]
    fn manages_todos_by_project_without_creating_status_events() {
        let first = Path::new("/first");
        let second = Path::new("/second");
        let mut library = Library::default();
        assert!(!library.add_todo(first, " \t "));
        assert!(library.annotation(first).is_none());
        assert!(library.add_todo(first, "  Ship it  "));
        assert!(library.add_todo(first, "Ship it"));
        assert!(library.add_todo(second, "Another project"));
        let tasks = library.annotation(first).unwrap().todos();
        let (a, b) = (tasks[0].id, tasks[1].id);
        assert_ne!(a, b);
        assert_eq!(tasks[0].title, "Ship it");
        assert!(!library.set_todo_completed(first, a, false));
        assert!(library.set_todo_completed(first, a, true));
        assert!(!library.annotation(second).unwrap().todos()[0].completed);
        assert!(!library.remove_todo(second, b));
        assert!(library.remove_todo(first, b));
        assert!(library.set_todo_completed(first, a, false));
        assert!(!library.annotation(first).unwrap().todos()[0].completed);
        assert!(
            library
                .annotation(first)
                .unwrap()
                .status_history()
                .is_empty()
        );
        assert!(library.remove_todo(first, a));
        assert!(library.annotation(first).is_none());
    }

    #[test]
    fn restores_committed_data_with_an_abandoned_staged_write() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store {
            path: temp.path().join("dashboard.json"),
        };
        let root = temp.path().join("workspace");
        let project = root.join("sample");
        let mut committed = Library::default();
        committed.set_favorite(&project, true);
        let report = Arc::new(ScanReport {
            root: root.clone(),
            projects: vec![],
            skipped_entries: 0,
            visited_directories: 1,
            elapsed_ms: 2,
        });
        committed.record_scan(report.clone());
        store.save(&committed)?;

        let mut pending = committed;
        pending.set_favorite(&project, false);
        pending.last_root = Some(temp.path().join("other-workspace"));
        // Abrupt termination skips both persist and the temporary file's destructor.
        let (_, orphan) = store.stage(&pending)?.keep()?;
        let restored = store.load()?;
        assert_eq!(restored.last_root, Some(root));
        assert_eq!(restored.snapshot(), Some(report));
        assert!(restored.is_favorite(&project));
        assert!(orphan.exists());
        Ok(())
    }

    #[test]
    fn reports_invalid_or_unsupported_data_without_rewriting_it() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store {
            path: temp.path().join("dashboard.json"),
        };
        for content in ["{ not valid JSON", "{\"schema_version\":42}"] {
            fs::write(store.path(), content)?;
            assert!(store.load().is_err());
            assert_eq!(fs::read_to_string(store.path())?, content);
        }
        Ok(())
    }
}
