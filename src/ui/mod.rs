mod board;
mod workspace;

use std::{borrow::Cow, collections::BTreeSet, path::PathBuf, sync::Arc, thread, time::Instant};

use anyhow::{Context, Result};
use project_analyser::{
    scanner::{ScanControl, ScanReport},
    storage::{Library, Store, Tag, path_key},
};
use serde::Deserialize;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Window, WindowBuilder},
};
use wry::{
    NewWindowResponse, WebView, WebViewBuilder,
    http::{Request, Response},
};

const URL: &str = "analyser://localhost/";
const WINDOWS_URL: &str = "http://analyser.localhost/";

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Command {
    Ready,
    Choose,
    Start,
    Cancel,
    RemoveFolder {
        id: String,
    },
    Export {
        report: String,
    },
    RetrySave,
    Board {
        request: board::Request,
    },
    Edit {
        id: String,
        #[serde(flatten)]
        edit: Edit,
    },
}

#[derive(Deserialize)]
#[serde(tag = "field", rename_all = "snake_case")]
enum Edit {
    Star { value: bool },
    Summary { value: String },
    Notes { value: String },
    Status { value: BTreeSet<Tag> },
    Tag { value: String, present: bool },
    Version { value: String, present: bool },
    AddTodo { value: String },
    ToggleTodo { task: u64, value: bool },
    RemoveTodo { task: u64 },
}

enum Message {
    Command(Command),
    Loaded(Result<(Store, Library)>),
    Progress(usize),
    Scanned(Result<Option<ScanReport>>),
    Saved(Result<()>),
}

#[derive(Default)]
struct Dashboard {
    library: Library,
    store: Option<Store>,
    loading: bool,
    ready: bool,
    saving: bool,
    dirty: bool,
    closing: bool,
    active: Option<Arc<[ScanControl]>>,
    scanned_folders: usize,
    notice: Option<String>,
    storage_error: Option<String>,
}

impl Dashboard {
    fn set_folders(&mut self, folders: Vec<PathBuf>, proxy: &EventLoopProxy<Message>) {
        self.library.scan_pending = !folders.is_empty();
        self.library.set_folders(folders);
        self.save(proxy);
    }

    fn choose(&mut self, window: &Window, proxy: &EventLoopProxy<Message>) {
        let dialog = rfd::FileDialog::new().set_title("Choose scan folders");
        #[cfg(not(target_os = "linux"))]
        let dialog = dialog.set_parent(window);
        #[cfg(target_os = "linux")]
        let _ = window; // RFD's GTK dialog does not use a raw window handle.
        if let Some(selected) = dialog.pick_folders() {
            let mut folders = self.library.folders().to_vec();
            self.notice = None;
            for path in selected {
                match dunce::canonicalize(&path) {
                    Ok(path) if !folders.iter().any(|root| path.starts_with(root)) => {
                        folders.retain(|root| !root.starts_with(&path));
                        folders.push(path);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.notice = Some(format!("Could not open {}: {error}", path.display()))
                    }
                }
            }
            if folders != self.library.folders() {
                self.set_folders(folders, proxy);
            }
        }
    }

    fn start(&mut self, proxy: &EventLoopProxy<Message>) {
        let folders = self.library.folders().to_vec();
        if self.active.is_some() || folders.is_empty() {
            return;
        }
        self.notice = None;
        self.scanned_folders = 0;
        self.library.scan_pending = true;
        self.save(proxy);
        let controls: Arc<[ScanControl]> =
            (0..folders.len()).map(|_| ScanControl::default()).collect();
        self.active = Some(Arc::clone(&controls));
        let proxy = proxy.clone();
        thread::spawn(move || {
            let result = workspace::scan(&folders, &controls, |done| {
                let _ = proxy.send_event(Message::Progress(done));
            });
            let _ = proxy.send_event(Message::Scanned(result));
        });
    }

    fn cancel(&self) {
        if let Some(active) = &self.active {
            for control in active.iter() {
                control.cancel();
            }
        }
    }

    fn save(&mut self, proxy: &EventLoopProxy<Message>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.dirty = true;
        if self.saving {
            return;
        }
        self.dirty = false;
        self.saving = true;
        let library = self.library.clone();
        let proxy = proxy.clone();
        thread::spawn(move || {
            let _ = proxy.send_event(Message::Saved(store.save(&library)));
        });
    }

    fn edit(&mut self, id: &str, edit: Edit, proxy: &EventLoopProxy<Message>) {
        if self.store.is_none() {
            return;
        }
        let Some(path) = workspace::project(&self.library, id, |project| project.path.clone())
        else {
            return;
        };
        let library = &mut self.library;
        let changed = match edit {
            Edit::Star { value } => library.set_favorite(&path, value),
            Edit::Notes { value } => library.set_notes(&path, value.chars().take(8_000).collect()),
            Edit::Tag { value, present } => library.set_custom_tag(&path, &value, present),
            Edit::Version { value, present } => library.set_version_label(&path, &value, present),
            Edit::AddTodo { value } => library.add_todo(&path, &value),
            Edit::ToggleTodo { task, value } => library.set_todo_completed(&path, task, value),
            Edit::RemoveTodo { task } => library.remove_todo(&path, task),
            Edit::Summary { .. } | Edit::Status { .. } => {
                let existing = library.annotation(&path).cloned().unwrap_or_default();
                let (description, tags) = match edit {
                    Edit::Summary { value } => (value.chars().take(8_000).collect(), existing.tags),
                    Edit::Status { value } => (existing.description, value),
                    _ => unreachable!(),
                };
                library.set_annotation(&path, description, tags, &existing.version)
            }
        };
        if changed {
            self.save(proxy);
        }
    }

    fn export(&mut self, window: &Window, report: String) {
        let dialog = rfd::FileDialog::new()
            .set_title("Export workspace statistics")
            .set_file_name("project-analyser-statistics.json")
            .add_filter("JSON", &["json"]);
        #[cfg(not(target_os = "linux"))]
        let dialog = dialog.set_parent(window);
        #[cfg(target_os = "linux")]
        let _ = window;
        if let Some(path) = dialog.save_file() {
            let save = || -> Result<()> {
                use std::io::Write as _;
                let mut file = tempfile::NamedTempFile::new_in(
                    path.parent().context("Missing export directory")?,
                )?;
                file.write_all(report.as_bytes())?;
                file.persist(&path)?;
                Ok(())
            };
            self.notice = save()
                .err()
                .map(|error| format!("Could not export statistics: {error:#}"));
        }
    }

    fn update(
        &mut self,
        message: Message,
        window: &Window,
        proxy: &EventLoopProxy<Message>,
        started: Instant,
    ) {
        match message {
            Message::Command(Command::Ready) if !self.ready => {
                self.ready = true;
                self.loading = true;
                if std::env::var_os("PROJECT_ANALYSER_LOG_STARTUP").is_some() {
                    eprintln!("Dashboard ready: {} ms", started.elapsed().as_millis());
                }
                let proxy = proxy.clone();
                thread::spawn(move || {
                    let result =
                        Store::for_user().and_then(|store| Ok((store.clone(), store.load()?)));
                    let _ = proxy.send_event(Message::Loaded(result));
                });
            }
            Message::Command(_) if self.loading || self.closing => {}
            Message::Command(command) => match command {
                Command::Ready => {}
                Command::Choose if self.active.is_none() => self.choose(window, proxy),
                Command::RemoveFolder { id } if self.active.is_none() => {
                    let mut folders = self.library.folders().to_vec();
                    folders.retain(|folder| path_key(folder) != id);
                    if folders != self.library.folders() {
                        self.set_folders(folders, proxy);
                    }
                }
                Command::Start => {
                    if self.library.folders().is_empty() {
                        self.choose(window, proxy);
                    }
                    self.start(proxy);
                }
                Command::Cancel => self.cancel(),
                Command::Export { report } => self.export(window, report),
                Command::RetrySave => self.save(proxy),
                Command::Edit { id, edit } => self.edit(&id, edit, proxy),
                _ => {}
            },
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok((store, library)) => {
                        self.store = Some(store);
                        self.library = library;
                        self.start(proxy);
                    }
                    Err(error) => {
                        self.storage_error = Some(format!(
                            "Local data is read-only for this session: {error:#}"
                        ))
                    }
                }
            }
            Message::Progress(done) => self.scanned_folders = done,
            Message::Scanned(result) => {
                let cancelled = self
                    .active
                    .take()
                    .is_some_and(|controls| controls.iter().any(ScanControl::is_cancelled));
                self.library.scan_pending = false;
                match result {
                    Ok(Some(report)) if !cancelled => {
                        self.library.record_workspace_scan(Arc::new(report))
                    }
                    Err(error) if !cancelled => {
                        self.notice = Some(format!("Scan failed · {error:#}"))
                    }
                    _ => self.notice = Some("Scan cancelled".into()),
                }
                self.save(proxy);
            }
            Message::Saved(result) => {
                self.saving = false;
                match result {
                    Ok(()) => {
                        self.storage_error = None;
                        if self.dirty {
                            self.save(proxy);
                        }
                    }
                    Err(error) => {
                        self.closing = false;
                        self.storage_error =
                            Some(format!("Could not save dashboard data: {error:#}"));
                    }
                }
            }
        }
    }

    fn publish(&self, view: &WebView) -> Result<()> {
        let mut state = workspace::state(
            &self.library,
            self.store.as_ref(),
            self.loading,
            self.active
                .as_ref()
                .map(|controls| (self.scanned_folders, controls[0].is_cancelled())),
            self.notice.as_deref(),
            self.storage_error.as_deref(),
        );
        state["saving"] = self.saving.into();
        send_json(view, "receive", &state)
    }
}

fn send_json(view: &WebView, receiver: &str, value: &impl serde::Serialize) -> Result<()> {
    let json = serde_json::to_string(value)?;
    view.evaluate_script(&format!(
        "window.{receiver}(JSON.parse({}))",
        serde_json::to_string(&json)?
    ))?;
    Ok(())
}

fn asset(request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let (status, mime, bytes): (u16, _, &[u8]) =
        match (request.method().as_str(), request.uri().path()) {
            ("GET", "/") => (
                200,
                "text/html; charset=utf-8",
                include_bytes!("web/index.html"),
            ),
            ("GET", "/mockup.css") => (
                200,
                "text/css; charset=utf-8",
                include_bytes!("web/mockup.css"),
            ),
            ("GET", "/layout.css") => (
                200,
                "text/css; charset=utf-8",
                include_bytes!("web/layout.css"),
            ),
            ("GET", "/catalog.js") => (
                200,
                "text/javascript; charset=utf-8",
                include_bytes!("web/catalog.js"),
            ),
            ("GET", "/ideas.js") => (
                200,
                "text/javascript; charset=utf-8",
                include_bytes!("web/ideas.js"),
            ),
            ("GET", "/ideas.css") => (
                200,
                "text/css; charset=utf-8",
                include_bytes!("web/ideas.css"),
            ),
            ("GET", "/app.js") => (
                200,
                "text/javascript; charset=utf-8",
                include_bytes!("web/app.js"),
            ),
            ("GET", "/statistics.js") => (
                200,
                "text/javascript; charset=utf-8",
                include_bytes!("web/statistics.js"),
            ),
            ("GET", "/inter.woff2") => (200, "font/woff2", include_bytes!("web/inter.woff2")),
            ("GET", "/jetbrains-mono.woff2") => (
                200,
                "font/woff2",
                include_bytes!("web/jetbrains-mono.woff2"),
            ),
            _ => (404, "text/plain", b"Not found"),
        };
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*") // Embedded public assets only.
        .header("X-Content-Type-Options", "nosniff")
        .body(Cow::Borrowed(bytes))
        .expect("static headers")
}

fn dashboard_url(url: &str) -> bool {
    matches!(url, URL | WINDOWS_URL)
}

pub fn run() -> Result<()> {
    let started = Instant::now();
    let events = EventLoopBuilder::<Message>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("Project Analyzer")
        .with_inner_size(LogicalSize::new(1440, 900))
        .with_min_inner_size(LogicalSize::new(800, 540))
        .build(&events)?;
    let proxy = events.create_proxy();
    let ipc = proxy.clone();
    let builder = WebViewBuilder::new()
        .with_url(URL)
        .with_incognito(true)
        .with_background_color((8, 9, 10, 255))
        .with_custom_protocol("analyser".into(), |_, request| asset(&request))
        .with_navigation_handler(|url| dashboard_url(&url))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_ipc_handler(move |request| {
            if dashboard_url(&request.uri().to_string())
                && request.body().len() <= 2_000_000
                && let Ok(command) = serde_json::from_str(request.body())
            {
                let _ = ipc.send_event(Message::Command(command));
            }
        });
    #[cfg(target_os = "linux")]
    let view = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(
            window
                .default_vbox()
                .context("Missing GTK window container")?,
        )?
    };
    #[cfg(not(target_os = "linux"))]
    let view = builder.build(&window)?;

    let mut app = Dashboard::default();
    events.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                app.cancel();
                if app.saving {
                    app.closing = true;
                } else {
                    *flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(Message::Command(Command::Board { request })) => {
                if app.ready && !app.loading && !app.closing {
                    let (reply, changed) = request.apply(&mut app.library, app.store.is_some());
                    let result = if changed {
                        app.save(&proxy);
                        app.publish(&view)
                    } else {
                        Ok(())
                    };
                    if let Err(error) = result.and(send_json(&view, "receiveBoard", &reply)) {
                        eprintln!("Could not update idea board: {error:#}");
                    }
                }
            }
            Event::UserEvent(message) => {
                app.update(message, &window, &proxy, started);
                if app.closing && !app.saving {
                    *flow = ControlFlow::Exit;
                } else if let Err(error) = app.publish(&view) {
                    eprintln!("Could not update dashboard: {error:#}");
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_are_typed_and_assets_are_allowlisted() {
        let cmd = r#"{"action":"edit","id":"project","field":"status","value":["in_review"]}"#;
        assert!(matches!(
            serde_json::from_str(cmd),
            Ok(Command::Edit {
                edit: Edit::Status { .. },
                ..
            })
        ));
        let board =
            r##"{"action":"board","request":{"token":7,"op":"preview","markdown":"# Idea"}}"##;
        assert!(
            matches!(serde_json::from_str(board), Ok(Command::Board { request }) if request.token == 7)
        );
        assert!(dashboard_url(URL));
        assert_eq!(
            asset(&Request::get("/dashboard.json").body(vec![]).unwrap()).status(),
            404
        );
    }
}
