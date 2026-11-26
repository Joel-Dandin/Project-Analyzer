//! GPUI application entry point.

mod assets;
mod ui;

use gpui::{
    AppContext as _, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::{Root, Theme, ThemeMode};
use ui::Dashboard;

fn main() {
    // Measure time to GPUI's first frame callback, excluding compilation.
    let started = std::time::Instant::now();
    let app = Application::new().with_assets(assets::Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        let bounds = Bounds::centered(None, size(px(1200.), px(780.)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(760.), px(540.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Project Analyser".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Dashboard::new(window, cx));
                window.on_next_frame(move |_, _| {
                    if std::env::var_os("PROJECT_ANALYSER_LOG_STARTUP").is_some() {
                        eprintln!("First frame: {} ms", started.elapsed().as_millis());
                    }
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            eprintln!("Could not open Project Analyser: {error:#}");
            cx.quit();
        } else {
            cx.activate(true);
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
