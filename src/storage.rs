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
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::scanner::ScanReport;

const SCHEMA_VERSION: u32 = 1;

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
}

impl Tag {
    pub const ALL: [Self; 7] = [
        Self::Planned,
        Self::InProgress,
        Self::Developing,
        Self::OnHold,
        Self::Completed,
        Self::Archived,
        Self::Cancelled,
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
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Annotation {
    pub description: String,
    pub tags: BTreeSet<Tag>,
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
}

impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_root: None,
            snapshot: None,
            annotations: Arc::new(BTreeMap::new()),
            favorites: Arc::new(BTreeSet::new()),
        }
    }
}

impl Library {
    pub fn annotation(&self, project: &Path) -> Option<&Annotation> {
        self.annotations.get(&path_key(project))
    }

    /// Returns whether anything changed. Empty annotations do not occupy a record.
    pub fn set_annotation(&mut self, project: &Path, annotation: Annotation) -> bool {
        let key = path_key(project);
        if annotation == Annotation::default() {
            return self.annotations.contains_key(&key)
                && Arc::make_mut(&mut self.annotations).remove(&key).is_some();
        }
        if self.annotations.get(&key) == Some(&annotation) {
            return false;
        }
        Arc::make_mut(&mut self.annotations).insert(key, annotation);
        true
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

    pub fn record_scan(&mut self, report: Arc<ScanReport>) {
        self.last_root = Some(report.root.clone());
        self.snapshot = Some(report);
    }
}

fn path_key(path: &Path) -> String {
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
        let data: Library = serde_json::from_reader(BufReader::new(file))
            .context("Could not parse dashboard.json")?;
        ensure!(
            data.schema_version == SCHEMA_VERSION,
            "Dashboard data has unsupported schema version {}",
            data.schema_version
        );
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
    fn round_trips_and_replaces_state() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store {
            path: temp.path().join("dashboard").join("dashboard.json"),
        };
        let root = temp.path().join("workspace");
        let project = root.join("sample");
        let mut data = store.load()?;
        data.last_root = Some(root.clone());
        let annotation = Annotation {
            description: "Demo API".to_owned(),
            tags: BTreeSet::from([Tag::Developing, Tag::OnHold]),
        };
        assert!(data.set_annotation(&project, annotation.clone()));
        assert!(!data.set_annotation(&project, annotation.clone()));
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
        assert!(cleared.set_annotation(&project, Annotation::default()));
        store.save(&cleared)?;
        let cleared = store.load()?;
        assert!(cleared.annotation(&project).is_none());
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
        let data = store.load()?;
        let project = Path::new("/project");
        assert!(!data.is_favorite(project));
        assert_eq!(
            data.annotation(project).unwrap().description,
            "Existing note"
        );
        Ok(())
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
