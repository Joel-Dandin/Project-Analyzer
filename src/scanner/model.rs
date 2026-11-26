use std::{
    borrow::Cow,
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub(super) const ANDROID: &str = "Android";
pub(super) const IOS: &str = "iOS";
pub(super) const GRADLE: &str = "Gradle";
pub(super) const GRADLE_KOTLIN: &str = "Gradle (Kotlin DSL)";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Frontend,
    Backend,
    Desktop,
    Mobile,
}

impl Role {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Frontend => "Front end",
            Self::Backend => "Back end",
            Self::Desktop => "Desktop",
            Self::Mobile => "Mobile",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RemoteLocation {
    pub host: String,
    /// The hosting path (owner, organization, or nested group), not an authenticated user.
    pub account: Option<String>,
    pub repository: String,
}

impl RemoteLocation {
    pub fn slug(&self) -> String {
        match &self.account {
            Some(account) => format!("{account}/{}", self.repository),
            None => self.repository.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Remote {
    pub name: String,
    // Preserve the v1 flat host/account/repository representation in dashboard.json.
    #[serde(flatten)]
    pub location: RemoteLocation,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub push_targets: BTreeSet<RemoteLocation>,
    pub has_tracking_refs: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Repository {
    pub remotes: Vec<Remote>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Component {
    pub path: PathBuf,
    pub roles: BTreeSet<Role>,
    pub stack: BTreeSet<String>,
    pub signals: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Project {
    pub path: PathBuf,
    pub roles: BTreeSet<Role>,
    pub stack: BTreeSet<String>,
    pub components: Vec<Component>,
    pub repository: Option<Repository>,
}

/// The primary type shown on a project card and counted in workspace statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectKind {
    Frontend,
    Backend,
    FullStack,
    Desktop,
    Android,
    Ios,
    AndroidAndIos,
    Mobile,
    Gradle,
    Tooling,
}

impl ProjectKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Frontend => "Front end",
            Self::Backend => "Back end",
            Self::FullStack => "Full stack",
            Self::Desktop => "Desktop",
            Self::Android => ANDROID,
            Self::Ios => IOS,
            Self::AndroidAndIos => "Android & iOS",
            Self::Mobile => "Mobile",
            Self::Gradle => GRADLE,
            Self::Tooling => "General / tooling",
        }
    }
}

impl Project {
    pub fn name(&self) -> Cow<'_, str> {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
    }

    pub fn is_gradle(&self) -> bool {
        self.is_gradle_groovy() || self.is_gradle_kotlin()
    }

    pub fn is_gradle_groovy(&self) -> bool {
        self.stack.contains(GRADLE)
    }

    pub fn is_gradle_kotlin(&self) -> bool {
        self.stack.contains(GRADLE_KOTLIN)
    }

    pub fn is_mobile(&self) -> bool {
        self.roles.contains(&Role::Mobile)
    }

    pub fn is_android(&self) -> bool {
        self.stack.contains(ANDROID)
    }

    pub fn is_ios(&self) -> bool {
        self.stack.contains(IOS)
    }

    pub fn classification(&self) -> ProjectKind {
        match (self.is_android(), self.is_ios()) {
            (true, true) => return ProjectKind::AndroidAndIos,
            (true, false) => return ProjectKind::Android,
            (false, true) => return ProjectKind::Ios,
            (false, false) => {}
        }
        if self.is_mobile() {
            ProjectKind::Mobile
        } else if self.roles.contains(&Role::Frontend) && self.roles.contains(&Role::Backend) {
            ProjectKind::FullStack
        } else if self.roles.contains(&Role::Desktop) {
            ProjectKind::Desktop
        } else if self.roles.contains(&Role::Backend) {
            ProjectKind::Backend
        } else if self.roles.contains(&Role::Frontend) {
            ProjectKind::Frontend
        } else if self.is_gradle() {
            ProjectKind::Gradle
        } else {
            ProjectKind::Tooling
        }
    }

    pub fn kind(&self) -> &'static str {
        self.classification().label()
    }

    pub fn relative_path(&self, root: &Path) -> &Path {
        match self.path.strip_prefix(root) {
            Ok(path) if path.as_os_str().is_empty() => Path::new("."),
            Ok(path) => path,
            Err(_) => &self.path,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScanReport {
    pub root: PathBuf,
    pub projects: Vec<Project>,
    pub visited_directories: usize,
    pub skipped_entries: usize,
    pub elapsed_ms: u64,
}
