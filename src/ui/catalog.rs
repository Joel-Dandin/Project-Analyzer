use std::{path::Path, sync::Arc};

use project_analyser::{
    scanner::{Project, ScanReport},
    storage::Library,
};

use super::stats::ReportStats;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ProjectFilter {
    #[default]
    All,
    Gradle,
    Mobile,
    Android,
    Ios,
}

impl ProjectFilter {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Gradle,
        Self::Mobile,
        Self::Android,
        Self::Ios,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All project types",
            Self::Gradle => "Gradle (non-mobile)",
            Self::Mobile => "Mobile (all platforms)",
            Self::Android => "Android",
            Self::Ios => "iOS",
        }
    }

    fn matches(self, project: &Project) -> bool {
        match self {
            Self::All => true,
            Self::Gradle => project.is_gradle() && !project.is_mobile(),
            Self::Mobile => project.is_mobile(),
            Self::Android => project.is_android(),
            Self::Ios => project.is_ios(),
        }
    }
}

/// Immutable search data is prepared with the report, off the UI thread.
pub(super) struct Catalog {
    pub report: Arc<ScanReport>,
    index: Vec<String>,
    pub stats: ReportStats,
}

impl Catalog {
    pub fn new(report: Arc<ScanReport>) -> Self {
        let stats = ReportStats::from_report(&report);
        let index = report
            .projects
            .iter()
            .map(|project| {
                let mut text = format!(
                    "{} {} {}",
                    project.name(),
                    project.path.display(),
                    project.kind()
                );
                for item in &project.stack {
                    text.push(' ');
                    text.push_str(item);
                }
                if let Some(repository) = &project.repository {
                    for remote in &repository.remotes {
                        text.push(' ');
                        text.push_str(&remote.location.slug());
                        text.push(' ');
                        text.push_str(&remote.location.host);
                        for target in &remote.push_targets {
                            text.push(' ');
                            text.push_str(&target.slug());
                            text.push(' ');
                            text.push_str(&target.host);
                        }
                    }
                }
                text.to_lowercase()
            })
            .collect();
        Self {
            report,
            index,
            stats,
        }
    }

    pub fn project(&self, index: usize) -> Option<&Project> {
        self.report.projects.get(index)
    }

    pub fn position(&self, path: &Path) -> Option<usize> {
        self.report
            .projects
            .binary_search_by(|project| project.path.as_path().cmp(path))
            .ok()
    }

    pub fn filter(
        &self,
        query: &str,
        library: &Library,
        favorites_only: bool,
        project_filter: ProjectFilter,
    ) -> Vec<usize> {
        let query = query.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        if words.is_empty() && !favorites_only && project_filter == ProjectFilter::All {
            return (0..self.index.len()).collect();
        }
        self.index
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                let project = &self.report.projects[i];
                let path = &project.path;
                if !project_filter.matches(project)
                    || (favorites_only && !library.is_favorite(path))
                {
                    return None;
                }
                let mut missing = words
                    .iter()
                    .filter(|word| !text.contains(**word))
                    .peekable();
                if missing.peek().is_none() {
                    return Some(i);
                }
                let annotation = library.annotation(path)?;
                let description = annotation.description.to_lowercase();
                let version = annotation.version.to_lowercase();
                missing
                    .all(|word| {
                        description.contains(*word)
                            || version.contains(*word)
                            || annotation
                                .tags
                                .iter()
                                .any(|tag| tag.label().to_ascii_lowercase().contains(*word))
                    })
                    .then_some(i)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf};

    use project_analyser::{
        scanner::{Remote, RemoteLocation, Repository, Role},
        storage::Tag,
    };

    use super::*;

    #[test]
    fn filters_by_stack_remote_and_annotations() {
        let root = PathBuf::from("workspace");
        let path = root.join("shop");
        let project = Project {
            path: path.clone(),
            roles: BTreeSet::from([Role::Frontend, Role::Backend]),
            stack: BTreeSet::from(["Spring Boot".to_owned(), "React".to_owned()]),
            components: Vec::new(),
            repository: Some(Repository {
                remotes: vec![Remote {
                    name: "origin".to_owned(),
                    location: RemoteLocation {
                        host: "example.com".to_owned(),
                        account: Some("team".to_owned()),
                        repository: "shop".to_owned(),
                    },
                    push_targets: BTreeSet::from([RemoteLocation {
                        host: "example.org".to_owned(),
                        account: Some("release".to_owned()),
                        repository: "shop".to_owned(),
                    }]),
                    has_tracking_refs: true,
                }],
            }),
        };
        let other = Project {
            path: root.join("storefront"),
            roles: BTreeSet::from([Role::Frontend]),
            stack: BTreeSet::from(["React".to_owned()]),
            components: Vec::new(),
            repository: None,
        };
        let catalog = Catalog::new(Arc::new(ScanReport {
            root,
            projects: vec![project, other],
            visited_directories: 1,
            skipped_entries: 0,
            elapsed_ms: 0,
        }));
        let mut library = Library::default();
        assert_eq!(
            catalog.filter(
                "Spring react example.org",
                &library,
                false,
                ProjectFilter::All
            ),
            [0]
        );
        assert_eq!(catalog.stats.full_stack(), 1);
        assert_eq!(catalog.stats.with_remotes, 1);
        assert_eq!(catalog.position(&path), Some(0));
        assert!(
            catalog
                .filter("archived", &library, false, ProjectFilter::All)
                .is_empty()
        );
        library.set_annotation(
            &path,
            "Quarterly promotion".into(),
            BTreeSet::from([Tag::Archived]),
            "v1.2.3-rc1",
        );
        assert_eq!(
            catalog.filter(
                "ARCHIVED promotion React V1.2.3-RC1",
                &library,
                false,
                ProjectFilter::All
            ),
            [0]
        );
        assert!(
            catalog
                .filter("ARCHIVED missing", &library, false, ProjectFilter::All)
                .is_empty()
        );
        assert_eq!(
            catalog.filter("react", &library, false, ProjectFilter::All),
            [0, 1]
        );
        assert!(
            catalog
                .filter("", &library, true, ProjectFilter::All)
                .is_empty()
        );
        assert!(library.set_favorite(&path, true));
        assert_eq!(catalog.filter("", &library, true, ProjectFilter::All), [0]);
        assert_eq!(
            catalog.filter("React", &library, true, ProjectFilter::All),
            [0]
        );
        assert_eq!(
            catalog.filter("archived", &library, true, ProjectFilter::All),
            [0]
        );
        assert!(
            catalog
                .filter("storefront", &library, true, ProjectFilter::All)
                .is_empty()
        );
    }

    #[test]
    fn filters_gradle_and_mobile_platforms_independently_of_search_and_favorites() {
        let project = |name: &str, stacks: &[&str], roles: &[Role]| Project {
            path: Path::new("workspace").join(name),
            roles: roles.iter().copied().collect(),
            stack: stacks.iter().map(|item| (*item).to_owned()).collect(),
            components: Vec::new(),
            repository: None,
        };
        let catalog = Catalog::new(Arc::new(ScanReport {
            root: "workspace".into(),
            projects: vec![
                project("api", &["Gradle", "Spring Boot"], &[Role::Backend]),
                project(
                    "droid",
                    &["Gradle (Kotlin DSL)", "Android"],
                    &[Role::Mobile],
                ),
                project("flutter", &["Flutter"], &[Role::Mobile]),
                project("phone", &["iOS"], &[Role::Mobile]),
                project("shared", &["Android", "iOS", "Flutter"], &[Role::Mobile]),
            ],
            visited_directories: 0,
            skipped_entries: 0,
            elapsed_ms: 0,
        }));
        let mut library = Library::default();
        assert!(library.set_favorite(&catalog.report.projects[4].path, true));
        assert_eq!(
            catalog.filter("", &library, false, ProjectFilter::Gradle),
            [0]
        );
        assert_eq!(
            catalog.filter("", &library, false, ProjectFilter::Mobile),
            [1, 2, 3, 4]
        );
        assert_eq!(
            catalog.filter("", &library, false, ProjectFilter::Android),
            [1, 4]
        );
        assert_eq!(
            catalog.filter("", &library, false, ProjectFilter::Ios),
            [3, 4]
        );
        assert_eq!(
            catalog.filter("flutter", &library, true, ProjectFilter::Mobile),
            [4]
        );
        assert_eq!(catalog.stats.kinds["Android"], 1);
        assert_eq!(catalog.stats.kinds["Android & iOS"], 1);
        assert_eq!(catalog.stats.kinds["iOS"], 1);
    }
}
