use std::collections::BTreeSet;

use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::Input,
};
use project_analyser::{scanner::Project, storage::Tag};

use super::super::HISTORY_PAGE_SIZE;
use super::{CANVAS, Dashboard, muted, section, section_title};

impl Dashboard {
    pub(super) fn todos(&self, project: &Project, cx: &mut Context<Self>) -> impl IntoElement {
        let tasks = self
            .library
            .annotation(&project.path)
            .map(|entry| entry.todos())
            .unwrap_or_default();
        let completed = tasks.iter().filter(|task| task.completed).count();
        let can_edit = self.can_edit_todos(&project.path);
        let composer = self
            .todo_draft
            .as_ref()
            .filter(|draft| draft.project == project.path);

        section()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(section_title("Project to-dos"))
                    .when(!tasks.is_empty(), |el| {
                        el.child(muted(format!("{completed} of {} done", tasks.len())))
                    })
                    .when(composer.is_none(), |el| {
                        el.child(
                            Button::new("new-todo")
                                .outline()
                                .small()
                                .label("New to-do")
                                .disabled(!can_edit)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_todo_draft(window, cx)
                                })),
                        )
                    }),
            )
            .when_some(composer, |el, draft| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&draft.input).disabled(!can_edit)),
                        )
                        .child(
                            Button::new("save-todo")
                                .primary()
                                .small()
                                .label("Add")
                                .disabled(!can_edit)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.save_todo(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("close-todo-draft")
                                .outline()
                                .small()
                                .label("Close")
                                .on_click(cx.listener(|this, _, _, cx| this.close_todo_draft(cx))),
                        ),
                )
            })
            .when(tasks.is_empty(), |el| {
                el.child(muted("No to-dos for this project yet."))
            })
            .children(tasks.iter().map(|task| {
                let id = task.id;
                let toggle_path = project.path.clone();
                let remove_path = toggle_path.clone();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .rounded_md()
                    .bg(rgb(CANVAS))
                    .p_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Checkbox::new(("todo-completed", id))
                                .label(task.title.clone())
                                .checked(task.completed)
                                .disabled(!can_edit)
                                .on_click(cx.listener(move |this, checked, _, cx| {
                                    this.set_todo_completed(&toggle_path, id, *checked, cx)
                                })),
                        ),
                    )
                    .child(
                        Button::new(("remove-todo", id))
                            .outline()
                            .small()
                            .label("Remove")
                            .tooltip(format!("Remove to-do: {}", task.title))
                            .disabled(!can_edit)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_todo(&remove_path, id, cx)
                            })),
                    )
            }))
    }

    pub(super) fn status_history(
        &self,
        project: &Project,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let changes = self
            .library
            .annotation(&project.path)
            .map(|entry| entry.status_history())
            .unwrap_or_default();
        section()
            .child(section_title("Status history"))
            .when(changes.is_empty(), |el| {
                el.child(muted("No status changes recorded yet. Changes appear here when you save status tags."))
            })
            .children(changes.iter().rev().take(self.history_limit).map(|change| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .rounded_md()
                    .bg(rgb(CANVAS))
                    .p_2()
                    .child(muted(change.at.format("%Y-%m-%d %H:%M:%S UTC").to_string()))
                    .child(div().text_sm().child(format!(
                        "{} → {}",
                        status_labels(&change.previous),
                        status_labels(&change.current),
                    )))
            }))
            .when(changes.len() > self.history_limit, |el| {
                el.child(
                    Button::new("more-status-history")
                        .outline()
                        .small()
                        .label("Show older changes")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.history_limit = this.history_limit.saturating_add(HISTORY_PAGE_SIZE);
                            cx.notify();
                        })),
                )
            })
    }
}

fn status_labels(tags: &BTreeSet<Tag>) -> String {
    if tags.is_empty() {
        "No status".to_owned()
    } else {
        tags.iter()
            .map(|tag| tag.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
