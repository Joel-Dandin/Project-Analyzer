use std::{path::Path, sync::Arc};

use project_analyser::{
    scanner::{Project, ScanReport},
    storage::Library,
};

use super::stats::ReportStats;

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

    pub fn filter(&self, query: &str, library: &Library, favorites_only: bool) -> Vec<usize> {
        let query = query.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        if words.is_empty() && !favorites_only {
            return (0..self.index.len()).collect();
        }
        self.index
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                let path = &self.report.projects[i].path;
                if favorites_only && !library.is_favorite(path) {
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
                missing
                    .all(|word| {
                        description.contains(*word)
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
        storage::{Annotation, Tag},
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
            catalog.filter("Spring react example.org", &library, false),
            [0]
        );
        assert_eq!(catalog.stats.full_stack(), 1);
        assert_eq!(catalog.stats.with_remotes, 1);
        assert_eq!(catalog.position(&path), Some(0));
        assert!(catalog.filter("archived", &library, false).is_empty());
        library.set_annotation(
            &path,
            Annotation {
                description: "Quarterly promotion".to_owned(),
                tags: BTreeSet::from([Tag::Archived]),
            },
        );
        assert_eq!(
            catalog.filter("ARCHIVED promotion React", &library, false),
            [0]
        );
        assert!(
            catalog
                .filter("ARCHIVED missing", &library, false)
                .is_empty()
        );
        assert_eq!(catalog.filter("react", &library, false), [0, 1]);
        assert!(catalog.filter("", &library, true).is_empty());
        assert!(library.set_favorite(&path, true));
        assert_eq!(catalog.filter("", &library, true), [0]);
        assert_eq!(catalog.filter("React", &library, true), [0]);
        assert_eq!(catalog.filter("archived", &library, true), [0]);
        assert!(catalog.filter("storefront", &library, true).is_empty());
    }
}
