//! Read-only, bounded-concurrency project inventory.

mod files;
mod git;
mod group;
mod model;
mod rules;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
    },
    thread,
    time::Instant,
};

use anyhow::{Context as _, Result, anyhow, ensure};
use ignore::{DirEntry, WalkBuilder, WalkState};

use self::{
    files::is_link,
    rules::{Candidate, Evidence, Rule},
};
pub use model::{Component, Project, Remote, RemoteLocation, Repository, Role, ScanReport};

#[derive(Default)]
pub struct ScanControl {
    cancelled: AtomicBool,
    visited: AtomicUsize,
    classified: AtomicUsize,
    candidates: AtomicUsize,
    phase: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Walking,
    Classifying,
    Organizing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    pub visited_directories: usize,
    pub classified_directories: usize,
    pub candidate_directories: usize,
}

impl ScanControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Relaxed)
    }

    pub fn progress(&self) -> Progress {
        let phase = match self.phase.load(Relaxed) {
            1 => Phase::Classifying,
            2 => Phase::Organizing,
            _ => Phase::Walking,
        };
        Progress {
            phase,
            visited_directories: self.visited.load(Relaxed),
            classified_directories: self.classified.load(Relaxed),
            candidate_directories: self.candidates.load(Relaxed),
        }
    }
}

/// Use a fresh control per invocation. `Ok(None)` means cancellation, not an empty workspace.
pub fn scan(root: &Path, control: &ScanControl) -> Result<Option<ScanReport>> {
    if control.is_cancelled() {
        return Ok(None);
    }
    let start = Instant::now();
    let root =
        dunce::canonicalize(root).with_context(|| format!("Could not open {}", root.display()))?;
    fs::read_dir(&root).with_context(|| format!("Could not read folder {}", root.display()))?;

    let workers = thread::available_parallelism()
        .map_or(2, usize::from)
        .clamp(2, 8);
    let batches = walk(&root, workers, control);
    if control.is_cancelled() {
        return Ok(None);
    }

    let mut candidates = BTreeMap::<PathBuf, Vec<Candidate>>::new();
    let mut markers = BTreeMap::<PathBuf, PathBuf>::new();
    let mut skipped_entries = 0;
    for batch in batches {
        skipped_entries += batch.errors;
        for candidate in batch.candidates {
            if let Some(directory) = candidate.directory() {
                // A selected source-set or Xcode bundle can have a logical owner outside the scan.
                let directory = if directory.starts_with(&root) {
                    directory
                } else {
                    &root
                };
                candidates
                    .entry(directory.to_path_buf())
                    .or_default()
                    .push(candidate);
            }
        }
        for marker in batch.git_markers {
            if let Some(parent) = marker.parent() {
                markers.insert(parent.to_path_buf(), marker);
            }
        }
    }
    let jobs: Vec<_> = candidates.into_iter().collect();
    control.candidates.store(jobs.len(), Relaxed);
    control.phase.store(1, Relaxed);
    let (evidence, failures) = classify(&jobs, workers, control)?;
    skipped_entries += failures;
    if control.is_cancelled() {
        return Ok(None);
    }

    control.phase.store(2, Relaxed);
    let git_roots: BTreeSet<_> = markers.keys().cloned().collect();
    let mut projects = group::projects(evidence, &git_roots);
    let inherited = if !markers.contains_key(&root)
        && projects
            .iter()
            .any(|project| !markers.contains_key(&project.path))
    {
        find_enclosing_repository(&root).map(|marker| git::repository(&marker))
    } else {
        None
    };
    if let Some((_, failures)) = &inherited {
        skipped_entries += failures;
    }
    for project in &mut projects {
        if control.is_cancelled() {
            return Ok(None);
        }
        if let Some(marker) = markers.get(&project.path) {
            let (repository, failures) = git::repository(marker);
            project.repository = Some(repository);
            skipped_entries += failures;
        } else if let Some((repository, _)) = &inherited {
            project.repository = Some(repository.clone());
        }
    }
    if control.is_cancelled() {
        return Ok(None);
    }
    Ok(Some(ScanReport {
        root,
        projects,
        skipped_entries,
        visited_directories: control.visited.load(Relaxed),
        elapsed_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
    }))
}

fn find_enclosing_repository(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .skip(1)
        .map(|path| path.join(".git"))
        .find(|marker| {
            fs::symlink_metadata(marker)
                .is_ok_and(|meta| !is_link(&meta) && (meta.is_file() || meta.is_dir()))
        })
}

#[derive(Default)]
struct Batch {
    candidates: Vec<Candidate>,
    git_markers: Vec<PathBuf>,
    errors: usize,
}

struct Collector<'a> {
    output: &'a Mutex<Vec<Batch>>,
    batch: Batch,
}

impl Collector<'_> {
    fn record(
        &mut self,
        entry: Result<DirEntry, ignore::Error>,
        control: &ScanControl,
    ) -> WalkState {
        if control.is_cancelled() {
            return WalkState::Quit;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                self.batch.errors += 1;
                return WalkState::Continue;
            }
        };
        self.batch.errors += usize::from(entry.error().is_some());
        let file_type = entry.file_type();
        if entry.depth() > 0 && entry.file_name() == ".git" {
            if file_type.is_some_and(|kind| kind.is_file() || kind.is_dir())
                && !entry_is_link(&entry)
            {
                self.batch.git_markers.push(entry.into_path());
            }
            return WalkState::Skip;
        }
        if file_type.is_some_and(|kind| kind.is_dir()) {
            if entry.depth() > 0
                && (prune_dir(entry.file_name().to_str().unwrap_or_default())
                    || entry_is_link(&entry))
            {
                return WalkState::Skip;
            }
            control.visited.fetch_add(1, Relaxed);
        } else if file_type.is_some_and(|kind| kind.is_file())
            && let Some(rule) = entry.file_name().to_str().and_then(Rule::from_name)
            && !entry_is_link(&entry)
        {
            self.batch.candidates.push(Candidate {
                path: entry.into_path(),
                rule,
            });
        }
        WalkState::Continue
    }
}

impl Drop for Collector<'_> {
    fn drop(&mut self) {
        if let Ok(mut output) = self.output.lock() {
            output.push(std::mem::take(&mut self.batch));
        }
    }
}

fn walk(root: &Path, workers: usize, control: &ScanControl) -> Vec<Batch> {
    let batches = Mutex::new(Vec::<Batch>::with_capacity(workers));
    WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .threads(workers)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || (!prune_dir(entry.file_name().to_str().unwrap_or_default())
                    && !entry_is_link(entry))
        })
        .build_parallel()
        .run(|| {
            let mut collector = Collector {
                output: &batches,
                batch: Batch::default(),
            };
            Box::new(move |entry| collector.record(entry, control))
        });
    batches.into_inner().unwrap_or_default()
}

fn entry_is_link(entry: &DirEntry) -> bool {
    entry.path_is_symlink()
        || (cfg!(windows) && entry.metadata().map_or(true, |meta| is_link(&meta)))
}

fn prune_dir(name: &str) -> bool {
    const PRUNED: &[&str] = &[
        "node_modules",
        "bower_components",
        "target",
        "dist",
        "build",
        "out",
        "vendor",
        "obj",
        "bin",
        ".venv",
        "venv",
        ".tox",
        ".nox",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".next",
        ".nuxt",
        ".svelte-kit",
        ".turbo",
        ".cache",
        ".gradle",
        ".dart_tool",
        ".build",
        ".idea",
        ".svn",
        ".hg",
        ".vs",
        ".pnpm-store",
        "coverage",
        "site-packages",
        "$Recycle.Bin",
        "System Volume Information",
    ];
    PRUNED
        .iter()
        .any(|ignored| name.eq_ignore_ascii_case(ignored))
}

type Classified = (Vec<(PathBuf, Evidence)>, usize);

fn classify(
    jobs: &[(PathBuf, Vec<Candidate>)],
    workers: usize,
    control: &ScanControl,
) -> Result<(BTreeMap<PathBuf, Evidence>, usize)> {
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers.min(jobs.len()) {
            let next = &next;
            handles.push(scope.spawn(move || -> Classified {
                let mut output = Vec::new();
                let mut failures = 0;
                while !control.is_cancelled() {
                    let Some((path, files)) = jobs.get(next.fetch_add(1, Relaxed)) else {
                        break;
                    };
                    let (result, errors) = rules::inspect(files, &control.cancelled);
                    failures += errors;
                    if !result.stack.is_empty() {
                        output.push((path.clone(), result));
                    }
                    control.classified.fetch_add(1, Relaxed);
                }
                (output, failures)
            }));
        }
        let mut output = BTreeMap::new();
        let mut failures = 0;
        for handle in handles {
            let (part, errors) = handle
                .join()
                .map_err(|_| anyhow!("A scanner worker stopped unexpectedly"))?;
            output.extend(part);
            failures += errors;
        }
        ensure!(
            control.is_cancelled() || control.classified.load(Relaxed) == jobs.len(),
            "Not all manifest candidates were inspected"
        );
        Ok((output, failures))
    })
}
