use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::skeleton::Skeleton;

use super::{
    BLUE, CANVAS, Dashboard, empty, eyebrow, kind_color, metric, muted, panel, section,
    section_title,
};

impl Dashboard {
    pub(super) fn statistics(&self) -> impl IntoElement {
        panel().flex_1().min_w_0().child(
            div()
                .id("workspace-statistics")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .gap_3()
                .child(eyebrow("WORKSPACE STATISTICS"))
                .child(muted(if self.cached {
                    "Last saved scan · counts include all projects in the workspace"
                } else {
                    "Counts include all projects in the workspace, regardless of search"
                }))
                .when_some(self.catalog.as_ref(), |view, catalog| {
                    let stats = &catalog.stats;
                    let report = &catalog.report;
                    view.child(
                        section().child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_3()
                                .child(metric(stats.projects.to_string(), "Projects"))
                                .child(metric(stats.git_projects.to_string(), "With local Git"))
                                .child(metric(stats.with_remotes.to_string(), "With remotes")),
                        ),
                    )
                    .child(
                        section()
                            .child(section_title("Project types"))
                            .when(stats.kinds.is_empty(), |view| {
                                view.child(muted("No projects found."))
                            })
                            .children(stats.kinds.iter().map(|(&kind, &count)| {
                                distribution_row(kind, count, stats.projects, kind_color(kind))
                            })),
                    )
                    .child(
                        section()
                            .child(section_title("Top technologies"))
                            .when(stats.top_stacks.is_empty(), |view| {
                                view.child(muted("No technologies detected."))
                            })
                            .children(stats.top_stacks.iter().map(|(name, count)| {
                                distribution_row(name, *count, stats.projects, BLUE)
                            })),
                    )
                    .child(
                        section()
                            .child(section_title("Last successful scan"))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_3()
                                    .child(metric(
                                        report.visited_directories.to_string(),
                                        "Folders visited",
                                    ))
                                    .child(metric(
                                        report.skipped_entries.to_string(),
                                        "Entries skipped",
                                    ))
                                    .child(metric(
                                        format!("{} ms", report.elapsed_ms),
                                        "Scan time",
                                    )),
                            ),
                    )
                })
                .when(
                    self.catalog.is_none() && (self.booting || self.active.is_some()),
                    |view| {
                        view.child(section().child(Skeleton::new().w_full().h(px(50.))))
                            .child(section().child(Skeleton::new().w(relative(0.65)).h(px(72.))))
                    },
                )
                .when(
                    self.catalog.is_none() && !self.booting && self.active.is_none(),
                    |view| {
                        view.child(empty(
                            "No statistics yet",
                            if self.root.is_some() {
                                "Use Refresh to scan this folder."
                            } else {
                                "Choose a folder to see its statistics."
                            },
                        ))
                    },
                ),
        )
    }
}

fn distribution_row(label: &str, count: usize, total: usize, color: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .child(div().flex_1().min_w_0().truncate().child(label.to_owned()))
                .child(div().flex_none().child(count.to_string())),
        )
        .child(
            div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(rgb(CANVAS))
                .child(
                    div()
                        .h_full()
                        .w(relative(count as f32 / total.max(1) as f32))
                        .rounded_full()
                        .bg(rgb(color)),
                ),
        )
}
