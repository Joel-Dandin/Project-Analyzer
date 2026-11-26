use std::{path::Path, sync::Arc};

use project_analyser::{
    scanner::{Project, ProjectKind, ScanReport},
    storage::Library,
};

use super::stats::ReportStats;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ProjectFilter {
    #[default]
    All,
    Kind(ProjectKind),
    AnyMobile,
    AnyAndroid,
    AnyIos,
    Gradle,
    GradleGroovy,
    GradleKotlin,
}

impl ProjectFilter {
    pub const GROUPS: [(&'static str, &'static [Self]); 3] = [
        (
            "Project types",
            &[
                Self::All,
                Self::Kind(ProjectKind::Frontend),
                Self::Kind(ProjectKind::Backend),
                Self::Kind(ProjectKind::FullStack),
                Self::Kind(ProjectKind::Desktop),
                Self::Kind(ProjectKind::Tooling),
            ],
        ),
        (
            "Mobile platforms",
            &[
                Self::AnyMobile,
                Self::AnyAndroid,
                Self::Kind(ProjectKind::Android),
                Self::AnyIos,
                Self::Kind(ProjectKind::Ios),
                Self::Kind(ProjectKind::AndroidAndIos),
                Self::Kind(ProjectKind::Mobile),
            ],
        ),
        (
            "Non-mobile Gradle builds",
            &[Self::Gradle, Self::GradleGroovy, Self::GradleKotlin],
        ),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All project types",
            Self::Kind(ProjectKind::Android) => "Android only",
            Self::Kind(ProjectKind::Ios) => "iOS only",
            Self::Kind(ProjectKind::Mobile) => "Other mobile",
            Self::Kind(kind) => kind.label(),
            Self::AnyMobile => "Mobile (all platforms)",
            Self::AnyAndroid => "Android (any)",
            Self::AnyIos => "iOS (any)",
            Self::Gradle => "Gradle (non-mobile)",
            Self::GradleGroovy => "Gradle (Groovy DSL)",
            Self::GradleKotlin => "Gradle (Kotlin DSL)",
        }
    }

    fn matches(self, project: &Project) -> bool {
        match self {
            Self::All => true,
            Self::Kind(kind) => project.classification() == kind,
            Self::AnyMobile => project.is_mobile(),
            Self::AnyAndroid => project.is_android(),
            Self::AnyIos => project.is_ios(),
            Self::Gradle => !project.is_mobile() && project.is_gradle(),
            Self::GradleGroovy => !project.is_mobile() && project.is_gradle_groovy(),
            Self::GradleKotlin => !project.is_mobile() && project.is_gradle_kotlin(),
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

    fn project(name: &str, roles: &[Role], stack: &[&str]) -> Project {
        Project {
            path: Path::new("workspace").join(name),
            roles: roles.iter().copied().collect(),
            stack: stack.iter().map(|item| (*item).to_owned()).collect(),
            components: Vec::new(),
            repository: None,
        }
    }

    fn catalog_with(projects: Vec<Project>) -> Catalog {
        Catalog::new(Arc::new(ScanReport {
            root: "workspace".into(),
            projects,
            visited_directories: 0,
            skipped_entries: 0,
            elapsed_ms: 0,
        }))
    }

    fn assert_type_matches(catalog: &Catalog, cases: &[(ProjectFilter, &[usize])]) {
        let library = Library::default();
        for &(filter, expected) in cases {
            assert_eq!(
                catalog.filter("", &library, false, filter).as_slice(),
                expected,
                "{filter:?}"
            );
        }
    }

    #[test]
    fn filters_by_primary_project_type() {
        use ProjectFilter::Kind;
        use ProjectKind::{Backend, Desktop, Frontend, FullStack, Tooling};

        let catalog = catalog_with(vec![
            project("api", &[Role::Backend], &["FastAPI"]),
            project("desktop", &[Role::Frontend, Role::Desktop], &["Tauri"]),
            project("shop", &[Role::Frontend, Role::Backend], &["React", "Axum"]),
            project("site", &[Role::Frontend], &["React"]),
            project("tools", &[], &["Rust"]),
        ]);
        assert_type_matches(
            &catalog,
            &[
                (ProjectFilter::All, &[0, 1, 2, 3, 4]),
                (Kind(Backend), &[0]),
                (Kind(Desktop), &[1]),
                (Kind(FullStack), &[2]),
                (Kind(Frontend), &[3]),
                (Kind(Tooling), &[4]),
            ],
        );
    }

    #[test]
    fn filters_mobile_by_detected_platforms_and_combines_with_search_and_favorites() {
        use ProjectFilter::{AnyAndroid, AnyIos, AnyMobile, Kind};
        use ProjectKind::{Android, AndroidAndIos, Ios, Mobile};

        let catalog = catalog_with(vec![
            project(
                "droid",
                &[Role::Mobile],
                &["Android", "Gradle (Kotlin DSL)"],
            ),
            project("flutter", &[Role::Mobile], &["Flutter"]),
            project("phone", &[Role::Mobile], &["iOS"]),
            project("shared", &[Role::Mobile], &["Android", "iOS", "Flutter"]),
        ]);
        assert_type_matches(
            &catalog,
            &[
                (AnyMobile, &[0, 1, 2, 3]),
                (AnyAndroid, &[0, 3]),
                (AnyIos, &[2, 3]),
                (Kind(Android), &[0]),
                (Kind(Ios), &[2]),
                (Kind(AndroidAndIos), &[3]),
                (Kind(Mobile), &[1]),
            ],
        );
        let mut library = Library::default();
        assert!(library.set_favorite(&catalog.report.projects[3].path, true));
        assert_eq!(
            catalog.filter("FLUTTER", &library, true, Kind(AndroidAndIos)),
            [3]
        );
        assert!(
            catalog
                .filter("FLUTTER", &library, true, Kind(Mobile))
                .is_empty()
        );
        assert!(
            catalog
                .filter("native", &library, true, Kind(AndroidAndIos))
                .is_empty()
        );
        assert_eq!(catalog.stats.kinds["Android"], 1);
        assert_eq!(catalog.stats.kinds["Android & iOS"], 1);
        assert_eq!(catalog.stats.kinds["iOS"], 1);
    }

    #[test]
    fn filters_non_mobile_gradle_by_dsl_even_when_it_has_another_primary_type() {
        use ProjectFilter::{Gradle, GradleGroovy, GradleKotlin};

        let catalog = catalog_with(vec![
            project("backend", &[Role::Backend], &["Gradle", "Spring Boot"]),
            project("kotlin", &[], &["Gradle (Kotlin DSL)"]),
            project("mixed", &[], &["Gradle", "Gradle (Kotlin DSL)"]),
            project(
                "mobile",
                &[Role::Mobile],
                &["Android", "Gradle", "Gradle (Kotlin DSL)"],
            ),
            project("tools", &[], &["Rust"]),
        ]);
        assert_type_matches(
            &catalog,
            &[
                (Gradle, &[0, 1, 2]),
                (GradleGroovy, &[0, 2]),
                (GradleKotlin, &[1, 2]),
            ],
        );
    }
}
