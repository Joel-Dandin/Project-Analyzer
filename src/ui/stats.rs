use std::collections::BTreeMap;

use project_analyser::scanner::ScanReport;

const TOP_STACK_LIMIT: usize = 6;

/// Whole-workspace metrics, computed once with the catalog (not per render or search).
#[derive(Default)]
pub(super) struct ReportStats {
    pub projects: usize,
    pub kinds: BTreeMap<&'static str, usize>,
    pub git_projects: usize,
    pub with_remotes: usize,
    pub top_stacks: Vec<(String, usize)>,
}

impl ReportStats {
    pub fn from_report(report: &ScanReport) -> Self {
        let mut stats = Self {
            projects: report.projects.len(),
            ..Self::default()
        };
        let mut stacks = BTreeMap::<&str, usize>::new();
        for project in &report.projects {
            *stats.kinds.entry(project.kind()).or_default() += 1;
            if let Some(repository) = &project.repository {
                stats.git_projects += 1;
                stats.with_remotes += usize::from(!repository.remotes.is_empty());
            }
            for stack in &project.stack {
                *stacks.entry(stack).or_default() += 1;
            }
        }
        let mut ranked: Vec<_> = stacks.into_iter().collect();
        ranked.sort_unstable_by(|(a, a_count), (b, b_count)| b_count.cmp(a_count).then(a.cmp(b)));
        stats.top_stacks = ranked
            .into_iter()
            .take(TOP_STACK_LIMIT)
            .map(|(name, count)| (name.to_owned(), count))
            .collect();
        stats
    }

    pub fn full_stack(&self) -> usize {
        self.kinds.get("Full stack").copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf};

    use project_analyser::scanner::{Project, Remote, RemoteLocation, Repository, Role};

    use super::*;

    #[test]
    fn counts_projects_by_kind_and_git_and_ranks_stacks_per_project() {
        let root = PathBuf::from("work");
        let project = |name: &str, roles: &[Role], stack: &[&str], repository| Project {
            path: root.join(name),
            roles: roles.iter().copied().collect(),
            stack: stack.iter().map(|name| (*name).to_owned()).collect(),
            repository,
            components: vec![],
        };
        let remote = Remote {
            name: "origin".into(),
            location: RemoteLocation {
                host: "example.com".into(),
                account: None,
                repository: "app".into(),
            },
            push_targets: BTreeSet::new(),
            has_tracking_refs: false,
        };
        let mut report = ScanReport {
            root: root.clone(),
            projects: vec![
                project(
                    "app",
                    &[Role::Frontend, Role::Backend],
                    &["React", "Spring Boot"],
                    Some(Repository {
                        remotes: vec![
                            remote.clone(),
                            Remote {
                                name: "mirror".into(),
                                ..remote
                            },
                        ],
                    }),
                ),
                project("site", &[Role::Frontend], &["React", "TypeScript"], None),
                project("tools", &[], &["Rust"], Some(Repository::default())),
            ],
            visited_directories: 5,
            skipped_entries: 1,
            elapsed_ms: 12,
        };
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.projects, 3);
        assert_eq!(stats.full_stack(), 1);
        assert_eq!(stats.kinds.values().sum::<usize>(), 3);
        assert_eq!(stats.kinds["Front end"], 1);
        assert_eq!(stats.kinds["General / tooling"], 1);
        assert_eq!(stats.git_projects, 2);
        assert_eq!(stats.with_remotes, 1);
        assert_eq!(
            stats.top_stacks,
            [
                ("React", 2),
                ("Rust", 1),
                ("Spring Boot", 1),
                ("TypeScript", 1)
            ]
            .map(|(name, count)| (name.to_owned(), count))
        );
        report.projects.clear();
        let empty = ReportStats::from_report(&report);
        assert_eq!(empty.projects, 0);
        assert_eq!(empty.full_stack(), 0);
        assert!(empty.kinds.is_empty() && empty.top_stacks.is_empty());
    }
}
