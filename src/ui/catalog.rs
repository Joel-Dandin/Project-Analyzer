use std::{path::Path, sync::Arc};

use project_analyser::{
    scanner::{Project, ScanReport},
    storage::Library,
};

/// Immutable search data is prepared with the report, off the UI thread.
pub(super) struct Catalog {
    pub report: Arc<ScanReport>,
    index: Vec<String>,
    pub full_stack: usize,
    pub with_remotes: usize,
}

impl Catalog {
    pub fn new(report: Arc<ScanReport>) -> Self {
        let mut full_stack = 0;
        let mut with_remotes = 0;
        let index = report
            .projects
            .iter()
            .map(|project| {
                full_stack += usize::from(project.kind() == "Full stack");
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
                    with_remotes += usize::from(!repository.remotes.is_empty());
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
            full_stack,
            with_remotes,
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

    pub fn filter(&self, query: &str, library: &Library) -> Vec<usize> {
        let query = query.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        if words.is_empty() {
            return (0..self.index.len()).collect();
        }
        self.index
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                let mut missing = words
                    .iter()
                    .filter(|word| !text.contains(**word))
                    .peekable();
                if missing.peek().is_none() {
                    return Some(i);
                }
                let annotation = library.annotation(&self.report.projects[i].path)?;
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
        let catalog = Catalog::new(Arc::new(ScanReport {
            root,
            projects: vec![project],
            visited_directories: 1,
            skipped_entries: 0,
            elapsed_ms: 0,
        }));
        let mut library = Library::default();
        assert_eq!(catalog.filter("Spring react example.org", &library), [0]);
        assert_eq!(catalog.full_stack, 1);
        assert_eq!(catalog.with_remotes, 1);
        assert_eq!(catalog.position(&path), Some(0));
        assert!(catalog.filter("archived", &library).is_empty());
        library.set_annotation(
            &path,
            Annotation {
                description: "Quarterly promotion".to_owned(),
                tags: BTreeSet::from([Tag::Archived]),
            },
        );
        assert_eq!(catalog.filter("ARCHIVED promotion React", &library), [0]);
        assert!(catalog.filter("ARCHIVED missing", &library).is_empty());
    }
}
