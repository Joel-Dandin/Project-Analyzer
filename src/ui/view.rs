mod statistics;
mod tracking;

use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    input::Input,
    select::Select,
    skeleton::Skeleton,
};
use project_analyser::{scanner::Project, storage::Tag};

use crate::assets::LOGO;

use super::{Dashboard, catalog::ProjectFilter};

const CANVAS: u32 = 0x0e131d;
const PANEL: u32 = 0x141b28;
const RAISED: u32 = 0x1c2636;
const LINE: u32 = 0x28364a;
const INK: u32 = 0xeaf0f8;
const MUTED: u32 = 0x9aaec4;
const MINT: u32 = 0x77dfc0;
const AMBER: u32 = 0xffca84;
const BLUE: u32 = 0xa7c7ff;
const VIOLET: u32 = 0xcdbbff;

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(INK))
            .font_family(".SystemUIFont")
            .text_size(px(14.))
            .child(self.header(cx))
            .child(self.workspace_strip(cx))
            .children(self.notices(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .px_5()
                    .py_4()
                    .child(self.inventory(cx))
                    .when(self.show_statistics, |view| view.child(self.statistics()))
                    .when(!self.show_statistics, |view| view.child(self.details(cx))),
            )
            .child(self.footer(cx))
    }
}

impl Dashboard {
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scanning = self.active.is_some();
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .py_4()
            .border_b_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(40.))
                    .rounded_lg()
                    .bg(rgb(0x173c38))
                    .child(svg().path(LOGO).size(px(27.)).text_color(rgb(MINT))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(21.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child("Project Analyser"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child("WORKSPACE INVENTORY"),
                    ),
            )
            .child(
                Button::new("toggle-statistics")
                    .outline()
                    .label(if self.show_statistics {
                        "Project details"
                    } else {
                        "Statistics"
                    })
                    .disabled(self.draft.is_some())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_statistics = !this.show_statistics;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("choose-workspace")
                    .outline()
                    .label(if self.picking {
                        "Opening…"
                    } else {
                        "Choose folder"
                    })
                    .disabled(self.booting || self.picking || scanning || self.draft.is_some())
                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
            )
            .when(scanning, |this| {
                this.child(
                    Button::new("cancel-scan")
                        .outline()
                        .label("Cancel scan")
                        .disabled(
                            self.active
                                .as_ref()
                                .is_some_and(|active| active.0.is_cancelled()),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_scan(cx))),
                )
            })
            .when(!scanning, |this| {
                this.child(
                    Button::new("refresh")
                        .primary()
                        .label("Refresh")
                        .disabled(
                            self.root.is_none()
                                || self.booting
                                || self.picking
                                || self.draft.is_some(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(root) = this.root.clone() {
                                this.start_scan(root, cx);
                            }
                        })),
                )
            })
    }

    fn workspace_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.root.as_ref().map_or_else(
            || "No folder selected".to_owned(),
            |path| path.display().to_string(),
        );
        let copy_path = path.clone();
        let (total, full_stack, git) = self
            .catalog
            .as_ref()
            .map(|catalog| {
                (
                    catalog.stats.projects.to_string(),
                    catalog.stats.full_stack().to_string(),
                    catalog.stats.with_remotes.to_string(),
                )
            })
            .unwrap_or_else(|| ("—".into(), "—".into(), "—".into()));
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_5()
            .mx_5()
            .mt_4()
            .p_4()
            .rounded_lg()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .flex_col()
                    .gap_1()
                    .child(eyebrow("SCAN ROOT"))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(path),
                    )
                    .child(div().text_xs().truncate().text_color(rgb(MUTED)).child(
                        if self.cached {
                            "Showing last saved results"
                        } else {
                            "Scanned projects are never modified"
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_4()
                    .child(metric(total, "Projects"))
                    .child(metric(full_stack, "Full stack"))
                    .child(metric(git, "With remotes")),
            )
            .when(self.root.is_some(), |this| {
                this.child(
                    Button::new("copy-workspace-path")
                        .outline()
                        .small()
                        .label("Copy root")
                        .tooltip(copy_path.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()));
                            this.info = Some("Workspace path copied.".into());
                            cx.notify();
                        })),
                )
            })
    }

    fn notices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut notices = Vec::new();
        if let Some(progress) = self.progress_label() {
            notices.push(banner(progress, MINT).into_any_element());
        }
        if let Some(error) = &self.scan_error {
            notices.push(banner(error.clone(), AMBER).into_any_element());
        }
        if let Some(error) = &self.storage_error {
            notices.push(
                banner(error.clone(), AMBER)
                    .when(self.allow_save && self.dirty, |this| {
                        this.child(
                            Button::new("retry-save")
                                .outline()
                                .small()
                                .label("Retry save")
                                .disabled(self.saving)
                                .on_click(cx.listener(|this, _, _, cx| this.flush(cx))),
                        )
                    })
                    .into_any_element(),
            );
        }
        if let Some(info) = &self.info {
            notices.push(banner(info.clone(), BLUE).into_any_element());
        }
        notices
    }

    fn inventory(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = match &self.catalog {
            Some(catalog) => format!(
                "{} of {}",
                self.visible.len(),
                catalog.report.projects.len()
            ),
            None => "—".into(),
        };
        let view = cx.weak_entity();
        let cold_loading = self.catalog.is_none() && (self.booting || self.active.is_some());
        panel()
            .w(relative(0.38))
            .flex_none()
            .min_w(px(245.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(
                                if self.favorites_only {
                                    "Favorites"
                                } else {
                                    "Projects"
                                },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_xs().text_color(rgb(MUTED)).child(count))
                                    .child(
                                        Button::new("favorites-filter")
                                            .outline()
                                            .small()
                                            .label(if self.favorites_only {
                                                "Show all"
                                            } else {
                                                "Favorites only"
                                            })
                                            .disabled(
                                                self.catalog.is_none() || self.draft.is_some(),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_favorites_filter(cx)
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        Input::new(&self.search)
                            .disabled(self.catalog.is_none() || self.draft.is_some()),
                    )
                    .child(
                        Select::new(&self.type_select)
                            .title_prefix("Type: ")
                            .w_full()
                            .disabled(self.catalog.is_none() || self.draft.is_some()),
                    ),
            )
            .when(cold_loading, |this| this.child(inventory_skeleton()))
            .when(!cold_loading && self.visible.is_empty(), |this| {
                let (heading, hint) = if self.root.is_none() {
                    (
                        "No workspace yet",
                        "Choose a folder to discover its projects.",
                    )
                } else if self.catalog.is_none() {
                    ("No results yet", "Use Refresh to scan this folder.")
                } else if self.favorites_only
                    || self.project_filter != ProjectFilter::All
                    || !self.query.trim().is_empty()
                {
                    (
                        "No matches",
                        "Change the search, project type, or favorites filter.",
                    )
                } else {
                    (
                        "No projects found",
                        "Look for a folder with source or project manifests.",
                    )
                };
                this.child(empty(heading, hint))
            })
            .when(!self.visible.is_empty(), |this| {
                this.child(
                    uniform_list("inventory-rows", self.visible.len(), move |range, _, cx| {
                        if let Some(view) = view.upgrade() {
                            let dashboard = view.read(cx);
                            range
                                .filter_map(|row| dashboard.visible.get(row).copied())
                                .filter_map(|index| {
                                    let project = dashboard.catalog.as_ref()?.project(index)?;
                                    Some(dashboard.row(index, project, view.downgrade()))
                                })
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        }
                    })
                    .track_scroll(self.list_scroll.clone())
                    .flex_1()
                    .min_h_0()
                    .w_full(),
                )
            })
    }

    fn row(
        &self,
        index: usize,
        project: &Project,
        view: WeakEntity<Self>,
    ) -> impl IntoElement + use<> {
        let kind = project.kind();
        let selected = self.selected == Some(index);
        let relative = self
            .root
            .as_deref()
            .map_or(project.path.as_path(), |root| project.relative_path(root));
        let path = relative.display().to_string();
        let annotation = self.library.annotation(&project.path);
        let tag = annotation.and_then(|a| a.tags.first().copied());
        let subtitle = match annotation.filter(|a| !a.version.is_empty()) {
            Some(entry) => format!("{kind} · {} · {path}", entry.version),
            None => format!("{kind} · {path}"),
        };
        let favorite = self.library.is_favorite(&project.path);
        let height = px(64.);
        div().h(height).w_full().px_2().py_1().child(
            div()
                .id(("inventory-row", index))
                .flex()
                .size_full()
                .items_center()
                .gap_3()
                .px_3()
                .rounded_md()
                .border_1()
                .border_color(if selected { rgb(0x36605b) } else { rgb(PANEL) })
                .bg(if selected { rgb(0x1b3539) } else { rgb(PANEL) })
                .when(self.draft.is_none(), |this| {
                    this.cursor_pointer()
                        .focusable()
                        .tab_stop(true)
                        .focus(|style| style.border_color(rgb(MINT)))
                        .hover(|style| style.bg(rgb(if selected { 0x214144 } else { RAISED })))
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| this.select(index, cx));
                            }
                        })
                        .on_key_down(move |event, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                let _ = view.update(cx, |this, cx| this.select(index, cx));
                            }
                        })
                })
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .size(px(32.))
                        .rounded_md()
                        .bg(rgb(RAISED))
                        .items_center()
                        .justify_center()
                        .font_weight(FontWeight::BOLD)
                        .text_xs()
                        .text_color(rgb(kind_color(kind)))
                        .child(kind_code(kind)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .when(favorite, |this| {
                                    this.child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .text_color(rgb(AMBER))
                                            .child("★"),
                                    )
                                })
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(project.name().into_owned()),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(subtitle),
                        ),
                )
                .children(tag.map(|tag| chip(tag.label().into(), tag_color(tag)))),
        )
    }

    fn details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(project) = self.selected_project() else {
            return panel()
                .flex_1()
                .min_w_0()
                .child(
                    if self.booting || (self.active.is_some() && self.catalog.is_none()) {
                        detail_skeleton().into_any_element()
                    } else {
                        empty(
                            "Project details",
                            "Select a project to see its stack, Git repository and notes.",
                        )
                        .into_any_element()
                    },
                )
                .into_any_element();
        };
        let path = project.path.display().to_string();
        let copy_path = path.clone();
        let favorite = self.library.is_favorite(&project.path);
        panel()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_5()
                    .py_4()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(eyebrow("PROJECT DETAILS"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(project.name().into_owned()),
                                    )
                                    .child(chip(project.kind().into(), kind_color(project.kind()))),
                            )
                            .child(
                                Button::new("copy-project-path")
                                    .outline()
                                    .small()
                                    .label("Copy path")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_path.clone(),
                                        ));
                                        this.info = Some("Project path copied.".into());
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(path),
                            )
                            .child(
                                Button::new("toggle-favorite")
                                    .outline()
                                    .small()
                                    .label(if favorite {
                                        "Remove favorite"
                                    } else {
                                        "Add favorite"
                                    })
                                    .disabled(
                                        !self.allow_save || self.picking || self.draft.is_some(),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_favorite(cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .id("details-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.detail_scroll)
                    .px_5()
                    .py_4()
                    .gap_4()
                    .child(self.annotation(project, cx))
                    .child(self.todos(project, cx))
                    .child(self.status_history(project, cx))
                    .child(self.technology(project))
                    .child(self.repository(project))
                    .child(self.components(project, cx)),
            )
            .into_any_element()
    }

    fn annotation(&self, project: &Project, cx: &mut Context<Self>) -> impl IntoElement {
        let annotation = self.library.annotation(&project.path);
        let tags = annotation.map(|annotation| &annotation.tags);
        let description = annotation
            .map(|annotation| annotation.description.as_str())
            .unwrap_or_default();
        let version = annotation
            .map(|annotation| annotation.version.as_str())
            .unwrap_or_default();
        let editing = self.draft.as_ref();
        section()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(section_title("Notes, version & status"))
                    .when(editing.is_none(), |this| {
                        this.child(
                            Button::new("edit-annotation")
                                .outline()
                                .small()
                                .label("Edit")
                                .disabled(!self.allow_save)
                                .on_click(cx.listener(|this, _, window, cx| this.edit(window, cx))),
                        )
                    }),
            )
            .when_some(editing, |this, draft| {
                this.child(eyebrow("DESCRIPTION"))
                    .child(Input::new(&self.description).w_full().h(px(118.)))
                    .child(eyebrow("VERSION TAG"))
                    .child(Input::new(&self.version).w_full())
                    .child(eyebrow("STATUS TAGS"))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .children(Tag::ALL.into_iter().map(|tag| {
                                let label = format!(
                                    "{}{}",
                                    if draft.tags.contains(&tag) {
                                        "✓ "
                                    } else {
                                        ""
                                    },
                                    tag.label()
                                );
                                let button = Button::new(("draft-tag", tag as usize))
                                    .small()
                                    .label(label)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.toggle_tag(tag, cx)),
                                    );
                                if draft.tags.contains(&tag) {
                                    button.primary()
                                } else {
                                    button.outline()
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("save-annotation")
                                    .primary()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                            )
                            .child(
                                Button::new("cancel-annotation")
                                    .outline()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                            ),
                    )
            })
            .when(editing.is_none(), |this| {
                this.child(
                    div()
                        .text_color(rgb(if description.is_empty() { MUTED } else { INK }))
                        .child(if description.is_empty() {
                            "No description yet.".to_owned()
                        } else {
                            description.to_owned()
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(chip(
                            if version.is_empty() {
                                "No version".into()
                            } else {
                                format!("Version: {version}").into()
                            },
                            if version.is_empty() { MUTED } else { VIOLET },
                        ))
                        .when(tags.is_none_or(|tags| tags.is_empty()), |this| {
                            this.child(chip("No status".into(), MUTED))
                        })
                        .children(
                            tags.into_iter()
                                .flatten()
                                .map(|tag| chip(tag.label().into(), tag_color(*tag))),
                        ),
                )
            })
    }

    fn technology(&self, project: &Project) -> impl IntoElement {
        section().child(section_title("Detected stack")).child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(
                    project
                        .roles
                        .iter()
                        .map(|role| chip(role.label().into(), MINT)),
                )
                .children(
                    project
                        .stack
                        .iter()
                        .map(|stack| chip(stack.clone().into(), INK)),
                ),
        )
    }

    fn repository(&self, project: &Project) -> impl IntoElement {
        section().child(section_title("Git repositories"))
            .when(project.repository.is_none(), |this| this.child(muted("No local Git repository detected.")))
            .when_some(project.repository.as_ref(), |this, repository| this
                .when(repository.remotes.is_empty(), |this| this.child(muted("Local Git repository · no supported network remote configured.")))
                .children(repository.remotes.iter().map(|remote| {
                    div().flex().flex_col().gap_1().p_3().rounded_md().bg(rgb(CANVAS))
                        .child(div().flex().items_center().flex_wrap().gap_2()
                            .child(chip(remote.name.clone().into(), BLUE))
                            .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(remote.location.slug())))
                        .child(div().text_xs().text_color(rgb(MUTED)).child(format!("{} · {}", remote.location.host,
                            if remote.has_tracking_refs { "Local remote-tracking refs found" } else { "Configured · no tracking refs found" })))
                        .children(remote.push_targets.iter().map(|target| {
                            muted(format!("Push destination: {} · {}", target.slug(), target.host))
                        }))
                }))
                .when(!repository.remotes.is_empty(), |this| this.child(muted("The host and owner are inferred from local remotes. Push status is not verified."))))
    }

    fn components(&self, project: &Project, cx: &mut Context<Self>) -> impl IntoElement {
        let limit = self.component_limit;
        section()
            .child(section_title(format!(
                "Detected folders · {}",
                project.components.len()
            )))
            .children(project.components.iter().take(limit).map(|component| {
                let path = component
                    .path
                    .strip_prefix(&project.path)
                    .unwrap_or(&component.path);
                let name = if path.as_os_str().is_empty() {
                    ". (project root)".into()
                } else {
                    path.display().to_string()
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).child(name))
                    .child(muted(
                        component
                            .stack
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" · "),
                    ))
                    .child(muted(format!(
                        "Evidence: {}",
                        component
                            .signals
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    )))
            }))
            .when(project.components.len() > limit, |this| {
                this.child(
                    Button::new("more-components")
                        .outline()
                        .small()
                        .label("Show more folders")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.component_limit = this
                                .component_limit
                                .saturating_add(super::COMPONENTS_PER_PAGE);
                            cx.notify();
                        })),
                )
            })
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = if self.saving {
            "Saving local data…".into()
        } else if self.booting {
            "Loading local data…".into()
        } else if let Some(catalog) = &self.catalog {
            let report = &catalog.report;
            let skipped = if report.skipped_entries == 0 {
                String::new()
            } else {
                format!(" · {} read issues", report.skipped_entries)
            };
            format!(
                "{} scan · {} folders · {} ms{skipped}",
                if self.cached { "Cached" } else { "Last" },
                report.visited_directories,
                report.elapsed_ms
            )
        } else {
            "Local · read-only scan".into()
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .py_2()
            .border_t_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .truncate()
                    .text_color(rgb(MUTED))
                    .child(status),
            )
            .when_some(self.store.as_ref(), |this, store| {
                let path = store.path().display().to_string();
                this.child(
                    Button::new("copy-data-path")
                        .ghost()
                        .small()
                        .label("Copy data file path")
                        .tooltip(path.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                            this.info = Some("Data file path copied.".into());
                            cx.notify();
                        })),
                )
            })
    }
}

fn panel() -> Div {
    div()
        .flex()
        .flex_col()
        .min_h_0()
        .overflow_hidden()
        .rounded_lg()
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
}

fn section() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap_2()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(RAISED))
}

fn eyebrow(label: &'static str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(label)
}

fn section_title(label: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

fn muted(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED)).child(text.into())
}

fn metric(amount: String, label: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .px_2()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(20.))
                .text_color(rgb(MINT))
                .child(amount),
        )
        .child(muted(label))
}

fn banner(label: String, color: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .mx_5()
        .mt_2()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
        .child(div().size(px(6.)).flex_none().rounded_full().bg(rgb(color)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(rgb(color))
                .child(label),
        )
}

fn chip(label: SharedString, color: u32) -> Div {
    div()
        .flex_none()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
        .text_xs()
        .text_color(rgb(color))
        .child(label)
}

fn empty(title: &'static str, hint: &'static str) -> Div {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .p_5()
        .text_center()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(muted(hint))
}

fn inventory_skeleton() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .children((0_usize..5).map(|index| {
            div()
                .id(("row-skeleton", index))
                .flex()
                .gap_3()
                .items_center()
                .p_3()
                .rounded_md()
                .bg(rgb(RAISED))
                .child(Skeleton::new().size(px(28.)).rounded_md())
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .id("skeleton-name")
                                .child(Skeleton::new().w(relative(0.64)).h(px(11.)).rounded_md()),
                        )
                        .child(
                            div().id("skeleton-label").child(
                                Skeleton::new()
                                    .secondary()
                                    .w(relative(0.44))
                                    .h(px(8.))
                                    .rounded_md(),
                            ),
                        ),
                )
        }))
}

fn detail_skeleton() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .child(
            div()
                .id("skeleton-title")
                .child(Skeleton::new().w(relative(0.44)).h(px(26.)).rounded_md()),
        )
        .child(
            div().id("skeleton-path").child(
                Skeleton::new()
                    .secondary()
                    .w(relative(0.62))
                    .h(px(12.))
                    .rounded_md(),
            ),
        )
        .child(
            div()
                .id("skeleton-notes")
                .child(Skeleton::new().h(px(94.)).rounded_lg()),
        )
        .child(
            div()
                .id("skeleton-stack")
                .child(Skeleton::new().secondary().h(px(52.)).rounded_lg()),
        )
}

fn kind_color(kind: &str) -> u32 {
    match kind {
        "Full stack" => MINT,
        "Back end" => VIOLET,
        "Front end" => BLUE,
        "Desktop" | "Mobile" => AMBER,
        "Android" => MINT,
        "iOS" => VIOLET,
        "Android & iOS" | "Gradle" => BLUE,
        _ => MUTED,
    }
}

fn kind_code(kind: &str) -> &'static str {
    match kind {
        "Full stack" => "FS",
        "Back end" => "BE",
        "Front end" => "FE",
        "Desktop" => "DT",
        "Mobile" => "MB",
        "Android" => "AN",
        "iOS" => "iOS",
        "Android & iOS" => "AI",
        "Gradle" => "GR",
        _ => "PR",
    }
}

fn tag_color(tag: Tag) -> u32 {
    match tag {
        Tag::Developing | Tag::Completed => MINT,
        Tag::InProgress | Tag::Planned => BLUE,
        Tag::OnHold | Tag::Cancelled => AMBER,
        Tag::Archived => MUTED,
    }
}
