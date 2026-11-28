//! Typed, on-demand reads, writes, and offline previews for the two idea boards.

use anyhow::{Context as _, Result, ensure};
use project_analyser::storage::{IdeaDraft, IdeaStatus, Library, MARKDOWN_LIMIT};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::workspace;

#[derive(Deserialize)]
pub(super) struct Request {
    pub token: u32,
    #[serde(flatten)]
    operation: Operation,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Operation {
    ReadScratchpad { id: String },
    SaveScratchpad { id: String, markdown: String },
    ReadIdea { id: u32 },
    SaveIdea { id: Option<u32>, draft: IdeaDraft },
    RemoveIdea { id: u32 },
    Preview { markdown: String },
}

#[derive(Serialize)]
pub(super) struct Reply {
    token: u32,
    value: Value,
    error: Option<String>,
}

impl Request {
    pub fn apply(self, library: &mut Library, editable: bool) -> (Reply, bool) {
        let (value, error, changed) = match self.operation.apply(library, editable) {
            Ok((value, changed)) => (value, None, changed),
            Err(error) => (Value::Null, Some(error.to_string()), false),
        };
        (
            Reply {
                token: self.token,
                value,
                error,
            },
            changed,
        )
    }
}

impl Operation {
    fn apply(self, library: &mut Library, editable: bool) -> Result<(Value, bool)> {
        let mut changed = false;
        let value = match self {
            Self::ReadScratchpad { id } => {
                workspace::project(library, &id, |p| json!(library.scratchpad(&p.path)))
                    .context("Project is no longer in the current scan.")?
            }
            Self::SaveScratchpad { id, markdown } => {
                ensure!(editable, "Local data is read-only.");
                let path = workspace::project(library, &id, |p| p.path.clone())
                    .context("Project is no longer in the current scan.")?;
                changed = library.set_scratchpad(&path, markdown);
                json!(library.scratchpad(&path))
            }
            Self::ReadIdea { id } => json!(library.idea(id).context("Idea no longer exists.")?),
            Self::SaveIdea { id, draft } => {
                ensure!(editable, "Local data is read-only.");
                ensure!(!draft.title.trim().is_empty(), "Give the idea a title.");
                if draft.status == IdeaStatus::Promoted {
                    let link = draft
                        .project_id
                        .as_deref()
                        .context("Choose a project to promote this idea.")?;
                    let existing = id
                        .and_then(|id| library.idea(id))
                        .and_then(|idea| idea.draft.project_id.as_deref());
                    ensure!(
                        existing == Some(link)
                            || workspace::project(library, link, |_| ()).is_some(),
                        "Choose a project from the current scan."
                    );
                }
                let (id, updated) = library
                    .save_idea(id, draft)
                    .context("Could not save this idea.")?;
                changed = updated;
                json!(library.idea(id))
            }
            Self::RemoveIdea { id } => {
                ensure!(editable, "Local data is read-only.");
                changed = library.remove_idea(id);
                ensure!(changed, "Idea no longer exists.");
                Value::Null
            }
            Self::Preview { markdown } => json!(preview(&markdown)),
        };
        Ok((value, changed))
    }
}

fn preview(markdown: &str) -> String {
    let markdown = if markdown.len() > MARKDOWN_LIMIT {
        markdown
            .char_indices()
            .nth(MARKDOWN_LIMIT)
            .map_or(markdown, |(end, _)| &markdown[..end])
    } else {
        markdown
    };
    // Render only Markdown-generated markup. HTML is escaped; links and images
    // keep their labels but cannot navigate or load any local or remote resource.
    let events = Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .filter_map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Some(Event::Text(raw)),
        Event::Start(Tag::Link { .. } | Tag::Image { .. })
        | Event::End(TagEnd::Link | TagEnd::Image) => None,
        safe => Some(safe),
    });
    let mut output = String::with_capacity(markdown.len());
    html::push_html(&mut output, events);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::Arc};

    #[test]
    fn previews_commonmark_with_no_active_html_resources_or_navigation() {
        let output = preview(
            "# Idea\n\n- [x] Test **bold**\n\n```rust\nlet n = 1 < 2;\n```\n\n[link](https://example.com) ![diagram](https://example.com/d.png)\n\n<b>raw</b>",
        );
        assert!(output.contains("<h1>Idea</h1>"));
        assert!(output.contains("<strong>bold</strong>"));
        assert!(output.contains("disabled"));
        assert!(output.contains("1 &lt; 2"));
        assert!(output.contains("link diagram"));
        assert!(output.contains("&lt;b&gt;raw&lt;/b&gt;"));
        assert!(!output.contains("href=") && !output.contains("src=") && !output.contains("<img"));
    }

    #[test]
    fn validates_project_links_and_allows_a_saved_link_to_an_unavailable_project() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("build.gradle.kts"), "plugins { java }")?;
        let root_path = dunce::canonicalize(root.path())?;
        let mut library = Library::default();
        let roots = [root_path.clone()];
        let snapshot = workspace::scan(&roots, &[Default::default()], |_| {})?.unwrap();
        library.set_folders(roots.to_vec());
        library.record_workspace_scan(Arc::new(snapshot));
        let id = project_analyser::storage::path_key(&root_path);
        let mut draft = IdeaDraft {
            title: "Idea".into(),
            markdown: "**Draft**".into(),
            status: IdeaStatus::Promoted,
            tags: Default::default(),
            project_id: Some(id.clone()),
        };
        assert!(
            Operation::SaveIdea {
                id: None,
                draft: draft.clone()
            }
            .apply(&mut library, false)
            .is_err()
        );
        assert!(
            Operation::SaveIdea {
                id: None,
                draft: draft.clone()
            }
            .apply(&mut library, true)?
            .1
        );
        assert!(
            Operation::SaveScratchpad {
                id: id.clone(),
                markdown: "# Scratchpad".into()
            }
            .apply(&mut library, true)?
            .1
        );
        assert_eq!(
            Operation::ReadScratchpad { id: id.clone() }
                .apply(&mut library, false)?
                .0,
            "# Scratchpad"
        );
        let idea_id = library.ideas()[0].id;
        library.set_folders(vec![]);
        assert!(
            Operation::ReadScratchpad { id }
                .apply(&mut library, true)
                .is_err()
        );
        draft.title = "Revised".into();
        assert!(
            Operation::SaveIdea {
                id: Some(idea_id),
                draft: draft.clone()
            }
            .apply(&mut library, true)?
            .1
        );
        assert!(
            Operation::SaveIdea { id: None, draft }
                .apply(&mut library, true)
                .is_err()
        );
        Ok(())
    }
}
