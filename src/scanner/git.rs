//! Inspect local Git metadata only. Never run Git, follow config includes, or contact a remote.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component as PathComponent, Path, PathBuf},
};

use url::Url;

use super::{
    files::{is_link, read_small_text},
    model::{Remote, RemoteLocation, Repository},
};

const CONFIG_LIMIT: u64 = 256 * 1024;
const REFS_LIMIT: u64 = 1024 * 1024;

pub(super) fn repository(marker: &Path) -> (Repository, usize) {
    match read_repository(marker) {
        Some(repository) => (repository, 0),
        None => (Repository::default(), 1),
    }
}

fn read_repository(marker: &Path) -> Option<Repository> {
    let metadata = fs::symlink_metadata(marker).ok()?;
    let git_dir = if metadata.is_dir() && !is_link(&metadata) {
        marker.to_path_buf()
    } else {
        let text = read_small_text(marker, 4096).ok()?;
        metadata_path(
            marker.parent()?,
            text.trim().strip_prefix("gitdir:")?.trim(),
        )?
    };
    if !plain_dir(&git_dir) {
        return None;
    }
    // Linked worktrees and submodules can point to metadata outside their working tree.
    let common = if let Ok(text) = read_small_text(&git_dir.join("commondir"), 4096) {
        metadata_path(&git_dir, text.trim())?
    } else {
        git_dir
    };
    if !plain_dir(&common) {
        return None;
    }
    let config = read_small_text(&common.join("config"), CONFIG_LIMIT).ok()?;
    let mut remotes = parse_config(&config);
    if !remotes.is_empty() {
        let packed = read_small_text(&common.join("packed-refs"), REFS_LIMIT).unwrap_or_default();
        let known_names: BTreeSet<_> = remotes.iter().map(|remote| remote.name.clone()).collect();
        let packed_remotes = packed_tracking_names(&packed, &known_names);
        for remote in &mut remotes {
            remote.has_tracking_refs = packed_remotes.contains(remote.name.as_str())
                || has_loose_refs(&common, &remote.name, &known_names);
        }
    }
    Some(Repository { remotes })
}

/// Resolve a Git pointer one local segment at a time. Reject links at intermediate levels and
/// prevent an untrusted pointer from opening an SMB share or a Windows device path.
fn metadata_path(base: &Path, value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let path = Path::new(value);
    #[cfg(windows)]
    {
        use std::path::Prefix;
        match path.components().next() {
            Some(PathComponent::Prefix(prefix))
                if !matches!(prefix.kind(), Prefix::Disk(_)) || !path.is_absolute() =>
            {
                return None;
            }
            Some(PathComponent::RootDir) => return None,
            _ => {}
        }
    }
    let mut resolved = if path.is_absolute() {
        PathBuf::new()
    } else {
        base.to_path_buf()
    };
    for part in path.components() {
        match part {
            PathComponent::Prefix(_) | PathComponent::RootDir => resolved.push(part.as_os_str()),
            PathComponent::CurDir => {}
            PathComponent::ParentDir => {
                resolved.pop();
            }
            PathComponent::Normal(name) => {
                resolved.push(name);
                if !plain_dir(&resolved) {
                    return None;
                }
            }
        }
    }
    Some(resolved)
}

#[derive(Default)]
struct RemoteConfig {
    fetch: Option<RemoteLocation>,
    push: BTreeSet<RemoteLocation>,
}

fn parse_config(config: &str) -> Vec<Remote> {
    let mut parsed = BTreeMap::<String, RemoteConfig>::new();
    let mut section = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            section = remote_section(line);
        } else if let Some(name) = &section
            && !line.starts_with([';', '#'])
            && let Some((key, value)) = line.split_once('=')
        {
            let key = key.trim();
            if !key.eq_ignore_ascii_case("url") && !key.eq_ignore_ascii_case("pushurl") {
                continue;
            }
            let Some(location) = config_value(value).as_deref().and_then(remote_location) else {
                continue;
            };
            let entry = parsed.entry(name.clone()).or_default();
            if key.eq_ignore_ascii_case("pushurl") {
                entry.push.insert(location);
            } else if entry.fetch.is_none() {
                entry.fetch = Some(location);
            }
        }
    }
    let mut remotes: Vec<_> = parsed
        .into_iter()
        .filter_map(|(name, config)| {
            let mut push_targets = config.push;
            let location = config.fetch.or_else(|| push_targets.first().cloned())?;
            // Keep only different destinations; most repositories push and fetch from the same host.
            push_targets.remove(&location);
            Some(Remote {
                name,
                location,
                push_targets,
                has_tracking_refs: false,
            })
        })
        .collect();
    if let Some(origin) = remotes.iter().position(|remote| remote.name == "origin") {
        remotes[..=origin].rotate_right(1);
    }
    remotes
}

fn remote_section(line: &str) -> Option<String> {
    let (section, _) = line.strip_prefix('[')?.split_once(']')?;
    let (prefix, name) = section.split_once([' ', '\t', '.'])?;
    if !prefix.eq_ignore_ascii_case("remote") {
        return None;
    }
    let name = config_value(name)?;
    safe_ref_name(&name).then_some(name)
}

fn config_value(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut quoted = false;
    let mut chars = text.trim().chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => break,
            '\\' if quoted => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                _ => return None,
            },
            _ => out.push(ch),
        }
    }
    if quoted {
        return None;
    }
    let value = out.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn remote_location(value: &str) -> Option<RemoteLocation> {
    if value.contains("://") {
        let url = Url::parse(value).ok()?;
        if !matches!(url.scheme(), "http" | "https" | "ssh" | "git" | "git+ssh") {
            return None;
        }
        // URL user info, query parameters, and fragments are deliberately discarded.
        return location(url.host_str()?, url.path());
    }
    let (endpoint, target) = if value.starts_with('[') || value.contains("@[") {
        let (endpoint, target) = value.split_once("]:")?;
        (&value[..endpoint.len() + 1], target)
    } else {
        value.split_once(':')?
    };
    let host = endpoint.rsplit_once('@').map_or(endpoint, |(_, host)| host);
    if (host.len() == 1 && !endpoint.contains('@'))
        || target.starts_with(':')
        || target.starts_with('\\')
    {
        // Avoid treating local drive letters and Git remote helpers as hosts.
        return None;
    }
    location(host, target.split(['?', '#']).next()?)
}

fn location(host: &str, path: &str) -> Option<RemoteLocation> {
    let host = host.trim();
    if host.is_empty()
        || !host
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b".-_[]:".contains(&ch))
    {
        return None;
    }
    let path = path.trim_matches('/');
    let (account, repository) = path
        .rsplit_once('/')
        .map_or((None, path), |(owner, repo)| (Some(owner), repo));
    let repository = repository.strip_suffix(".git").unwrap_or(repository);
    if !safe_segment(repository)
        || account.is_some_and(|account| !account.split('/').all(safe_segment))
    {
        return None;
    }
    Some(RemoteLocation {
        host: host.to_ascii_lowercase(),
        account: account.map(str::to_owned),
        repository: repository.to_owned(),
    })
}

fn safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !matches!(segment, "." | "..")
        && !segment.contains(['\\', '@', ':'])
        && !segment
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
}

fn safe_ref_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':', '*', '?', '[', '^', '~', ' '])
        && Path::new(name)
            .components()
            .all(|part| matches!(part, PathComponent::Normal(_)))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
        && !name.chars().any(char::is_control)
}

fn packed_tracking_names<'a>(packed: &'a str, known_names: &BTreeSet<String>) -> BTreeSet<&'a str> {
    packed
        .lines()
        .filter_map(|line| {
            let (hash, reference) = line.split_once(' ')?;
            if !is_oid(hash) {
                return None;
            }
            let suffix = reference.strip_prefix("refs/remotes/")?;
            // Use the most specific configured remote if names share a path prefix.
            for (split, _) in suffix.rmatch_indices('/') {
                let (name, branch) = (&suffix[..split], &suffix[split + 1..]);
                if known_names.contains(name) {
                    return (!branch.is_empty() && branch != "HEAD").then_some(name);
                }
            }
            None
        })
        .collect()
}

fn has_loose_refs(common: &Path, name: &str, known_names: &BTreeSet<String>) -> bool {
    let mut dir = common.join("refs");
    if !plain_dir(&dir) {
        return false;
    }
    dir.push("remotes");
    if !plain_dir(&dir) {
        return false;
    }
    let refs = dir.clone();
    for part in name.split('/') {
        dir.push(part);
        if !plain_dir(&dir) {
            return false;
        }
    }
    let base = dir.clone();
    let mut todo: Vec<(PathBuf, usize)> = vec![(dir, 0)];
    let mut inspected = 0;
    while let Some((path, depth)) = todo.pop() {
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                inspected += 1;
                if inspected > 256 {
                    return false;
                }
                let child = entry.path();
                if let Ok(meta) = fs::symlink_metadata(&child) {
                    if is_link(&meta) {
                        continue;
                    }
                    if meta.is_dir() && depth < 8 {
                        let is_other_remote = child.strip_prefix(&refs).is_ok_and(|relative| {
                            known_names.contains(&relative.to_string_lossy().replace('\\', "/"))
                        });
                        if !is_other_remote {
                            todo.push((child, depth + 1));
                        }
                    } else if meta.is_file()
                        && !(path == base && entry.file_name() == "HEAD")
                        && read_small_text(&child, 256).is_ok_and(|text| is_oid(text.trim()))
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() && !is_link(&meta))
}

fn is_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_user_info_and_supports_namespaces() {
        let text = "[remote \"upstream\"]\nurl = ssh://alex@gitlab.com/group/team/demo.git\n\
                    [remote \"origin\"]\nurl = https://user:secret@example.com/owner/repo.git?token=private\n\
                    pushurl = https://user:other-secret@example.org/publisher/demo.git\n\
                    pushurl = git@mirror:org/demo.git\n\
                    [include]\npath = ignored\n[remote \"local\"]\nurl = C:/src/demo\n";
        let remotes = parse_config(text);
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].location.slug(), "owner/repo");
        assert_eq!(remotes[0].location.host, "example.com");
        assert_eq!(
            remotes[0]
                .push_targets
                .iter()
                .map(RemoteLocation::slug)
                .collect::<Vec<_>>(),
            ["publisher/demo", "org/demo"]
        );
        assert_eq!(remotes[1].location.slug(), "group/team/demo");
        assert_eq!(
            remote_location("git@[::1]:org/repo.git")
                .unwrap()
                .repository,
            "repo"
        );
        let persisted = serde_json::to_string(&remotes).unwrap();
        assert!(!persisted.contains("secret"));
        assert_eq!(
            serde_json::from_str::<Vec<Remote>>(&persisted).unwrap(),
            remotes
        );
        assert!(remote_location("ext::git helper").is_none());
    }

    #[test]
    fn reads_worktree_and_ignores_symbolic_remote_head() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let base = temp.path();
        fs::create_dir_all(base.join("git/worktrees/demo"))?;
        fs::create_dir_all(base.join("worktree"))?;
        fs::create_dir_all(base.join("git/refs/remotes/origin"))?;
        fs::write(
            base.join("worktree/.git"),
            "gitdir: ../git/worktrees/demo\n",
        )?;
        fs::write(base.join("git/worktrees/demo/commondir"), "../..\n")?;
        fs::write(
            base.join("git/config"),
            "[remote \"origin\"]\nurl = git@github.com:org/repo.git\n",
        )?;
        fs::write(
            base.join("git/refs/remotes/origin/HEAD"),
            "ref: refs/remotes/origin/main\n",
        )?;
        let marker = base.join("worktree/.git");
        assert!(!repository(&marker).0.remotes[0].has_tracking_refs);
        fs::write(
            base.join("git/packed-refs"),
            format!("{} refs/remotes/origin/main\n", "f".repeat(40)),
        )?;
        assert!(repository(&marker).0.remotes[0].has_tracking_refs);
        Ok(())
    }

    #[test]
    fn packed_refs_are_assigned_to_the_most_specific_remote() {
        let names = BTreeSet::from(["origin".to_owned(), "origin/team".to_owned()]);
        let entries = format!("{} refs/remotes/origin/team/feature/demo\n", "a".repeat(40));
        assert_eq!(
            packed_tracking_names(&entries, &names),
            BTreeSet::from(["origin/team"])
        );
    }
}
