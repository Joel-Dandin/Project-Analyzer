use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MARKDOWN_LIMIT: usize = 32_000;
pub const IDEA_TITLE_LIMIT: usize = 120;

pub(super) fn truncate_markdown(markdown: &mut String) {
    if markdown.len() > MARKDOWN_LIMIT
        && let Some((end, _)) = markdown.char_indices().nth(MARKDOWN_LIMIT)
    {
        markdown.truncate(end);
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeaStatus {
    #[default]
    Raw,
    Exploring,
    Parked,
    Promoted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IdeaDraft {
    pub title: String,
    pub markdown: String,
    pub status: IdeaStatus,
    pub tags: BTreeSet<String>,
    pub project_id: Option<String>,
}

impl IdeaDraft {
    fn normalize(mut self) -> Option<Self> {
        self.title = self.title.trim().chars().take(IDEA_TITLE_LIMIT).collect();
        if self.title.is_empty() {
            return None;
        }
        truncate_markdown(&mut self.markdown);
        self.tags = self
            .tags
            .into_iter()
            .map(|tag| tag.trim().chars().take(64).collect::<String>())
            .filter(|tag| !tag.is_empty())
            .collect();
        self.tags = self.tags.into_iter().take(24).collect();
        if self.status != IdeaStatus::Promoted {
            self.project_id = None;
        } else if self.project_id.as_deref().is_none_or(str::is_empty) {
            return None;
        }
        Some(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Idea {
    pub id: u32,
    #[serde(flatten)]
    pub draft: IdeaDraft,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct IdeaBoard {
    next_id: u32,
    entries: Vec<Idea>,
}

impl IdeaBoard {
    pub(super) fn entries(&self) -> &[Idea] {
        &self.entries
    }

    pub(super) fn save(&mut self, id: Option<u32>, draft: IdeaDraft) -> Option<(u32, bool)> {
        let draft = draft.normalize()?;
        if let Some(id) = id {
            let idea = self.entries.iter_mut().find(|idea| idea.id == id)?;
            if idea.draft == draft {
                return Some((id, false));
            }
            idea.draft = draft;
            idea.updated_at = Utc::now();
            Some((id, true))
        } else {
            let id = self.next_id.checked_add(1)?;
            self.next_id = id;
            let now = Utc::now();
            self.entries.push(Idea {
                id,
                draft,
                created_at: now,
                updated_at: now,
            });
            Some((id, true))
        }
    }

    pub(super) fn remove(&mut self, id: u32) -> bool {
        let count = self.entries.len();
        self.entries.retain(|idea| idea.id != id);
        self.entries.len() != count
    }
}
