//! Resolve open actions from the inventory, never from a WebView-supplied URL or path.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use project_analyser::{scanner::RemoteLocation, storage::Library};
use serde::Deserialize;
use url::Url;

use super::workspace;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    Folder,
    Repository,
}

pub(super) enum Target {
    Folder { path: PathBuf, roots: Vec<PathBuf> },
    Repository(Url),
}

impl Action {
    pub fn resolve(self, library: &Library, id: &str) -> Result<Target> {
        workspace::project(library, id, |project| match self {
            Self::Folder => Ok(Target::Folder {
                path: project.path.clone(),
                roots: library.folders().to_vec(),
            }),
            Self::Repository => {
                let remotes = &project
                    .repository
                    .as_ref()
                    .context("No Git repository.")?
                    .remotes;
                let primary = remotes
                    .iter()
                    .find(|remote| remote.name == "origin")
                    .or_else(|| remotes.first());
                let url = primary
                    .and_then(|remote| browser_url(&remote.location))
                    .context("No browser URL is available for the primary remote.")?;
                Ok(Target::Repository(url))
            }
        })
        .context("Project is no longer in the current scan.")?
    }
}

impl Target {
    pub fn open(self) -> Result<()> {
        match self {
            Self::Folder { path, roots } => opener::open(folder_path(&path, &roots)?)
                .context("Could not open the project folder in the file manager."),
            Self::Repository(url) => opener::open_browser(url.as_str())
                .context("Could not open the repository in the default browser."),
        }
    }
}

fn folder_path(path: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let path = dunce::canonicalize(path).context("Project folder is unavailable.")?;
    ensure!(
        path.is_dir() && workspace::contains(roots, &path),
        "Project folder is no longer a directory inside the scan folders."
    );
    Ok(path)
}

fn browser_url(location: &RemoteLocation) -> Option<Url> {
    let host = match location.host.as_str() {
        "ssh.github.com" => "github.com",
        host => host,
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_[]:".contains(&byte))
        || (host.contains(':') && !(host.starts_with('[') && host.ends_with(']')))
    {
        return None;
    }
    let slug = location.slug();
    if location.repository.contains('/')
        || slug.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.contains(['\\', '?', '#', '@', ':'])
                || part.chars().any(|ch| ch.is_whitespace() || ch.is_control())
        })
    {
        return None;
    }
    let mut url = Url::parse(&format!("https://{host}/")).ok()?;
    url.set_path(&slug);
    // Reject paths shortened by URL dot-segment normalization, including escaped dots.
    (url.path().split('/').skip(1).count() == slug.split('/').count()).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::Arc};

    #[test]
    fn builds_credential_free_https_urls_and_rejects_invalid_targets() {
        let mut location = RemoteLocation {
            host: "ssh.github.com".into(),
            account: Some("org".into()),
            repository: "sample".into(),
        };
        assert_eq!(
            browser_url(&location).unwrap().as_str(),
            "https://github.com/org/sample"
        );
        location.host = "gitlab.example".into();
        location.account = Some("group/team".into());
        location.repository = "caf%C3%A9".into();
        assert_eq!(
            browser_url(&location).unwrap().as_str(),
            "https://gitlab.example/group/team/caf%C3%A9"
        );
        for bad in [
            "file:///C:/",
            "github.com@elsewhere",
            "github.com/path",
            "github.com:443",
        ] {
            location.host = bad.into();
            assert!(browser_url(&location).is_none(), "{bad}");
        }
        location.host = "github.com".into();
        for bad in [
            "../other",
            "query?token=secret",
            "frag#anchor",
            "%2e%2e",
            "a/b",
        ] {
            location.repository = bad.into();
            assert!(browser_url(&location).is_none(), "{bad}");
        }
    }

    #[test]
    fn resolves_only_scanned_projects_and_existing_contained_folders() -> Result<()> {
        let outer = tempfile::tempdir()?;
        let root = outer.path().join("A project with spaces");
        fs::create_dir_all(root.join(".git"))?;
        fs::write(root.join("build.gradle"), "plugins { id 'java' }")?;
        fs::write(
            root.join(".git/config"),
            "[remote \"other\"]\nurl = git@github.com:other/sample.git\n[remote \"origin\"]\nurl = https://user:secret@github.com/org/sample.git?token=hidden\n",
        )?;
        let root = dunce::canonicalize(root)?;
        let roots = [root.clone()];
        let snapshot = workspace::scan(&roots, &[Default::default()], |_| {})?.unwrap();
        let mut library = Library::default();
        library.set_folders(roots.to_vec());
        library.record_workspace_scan(Arc::new(snapshot));
        let id = project_analyser::storage::path_key(&root);
        let Target::Folder { path, roots } = Action::Folder.resolve(&library, &id)? else {
            panic!("not a folder");
        };
        assert_eq!(folder_path(&path, &roots)?, root);
        let Target::Repository(url) = Action::Repository.resolve(&library, &id)? else {
            panic!("not a URL");
        };
        assert_eq!(url.as_str(), "https://github.com/org/sample");
        assert!(
            Action::Folder
                .resolve(&library, "outside/not-scanned")
                .is_err()
        );
        assert!(folder_path(outer.path(), &roots).is_err());
        assert!(folder_path(&root.join("missing"), &roots).is_err());
        assert!(folder_path(&root.join("build.gradle"), &roots).is_err());
        library.set_folders(vec![]);
        assert!(Action::Folder.resolve(&library, &id).is_err());
        Ok(())
    }
}
