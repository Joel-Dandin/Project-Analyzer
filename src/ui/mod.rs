mod catalog;
mod stats;
mod view;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use gpui::{
    AppContext as _, Context, Entity, PathPromptOptions, ScrollHandle, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window, point, px,
};
use gpui_component::{
    IndexPath,
    input::{InputEvent, InputState},
    select::{SearchableVec, SelectEvent, SelectGroup, SelectItem, SelectState},
};
use project_analyser::{
    scanner::{Phase, Project, ScanControl, scan},
    storage::{Library, Store, TODO_LIMIT, Tag, VERSION_LIMIT},
};

use catalog::{Catalog, ProjectFilter};

type TypeOptions = SearchableVec<SelectGroup<ProjectFilter>>;

impl SelectItem for ProjectFilter {
    type Value = Self;

    fn title(&self) -> SharedString {
        self.label().into()
    }

    fn value(&self) -> &Self {
        self
    }
}

const DESCRIPTION_LIMIT: usize = 8_000;
const COMPONENTS_PER_PAGE: usize = 20;
const HISTORY_PAGE_SIZE: usize = 10;

struct ActiveScan(Arc<ScanControl>);

impl Drop for ActiveScan {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct Draft {
    project: PathBuf,
    tags: BTreeSet<Tag>,
}

struct TodoDraft {
    project: PathBuf,
    input: Entity<InputState>,
    _events: Subscription,
}

pub struct Dashboard {
    library: Library,
    store: Option<Store>,
    allow_save: bool,
    dirty: bool,
    saving: bool,
    storage_error: Option<String>,
    root: Option<PathBuf>,
    catalog: Option<Catalog>,
    pending_catalog: Option<Catalog>,
    cached: bool,
    booting: bool,
    picking: bool,
    active: Option<ActiveScan>,
    scan_error: Option<String>,
    info: Option<String>,
    show_statistics: bool,
    favorites_only: bool,
    project_filter: ProjectFilter,
    type_select: Entity<SelectState<TypeOptions>>,
    query: String,
    search: Entity<InputState>,
    description: Entity<InputState>,
    version: Entity<InputState>,
    visible: Vec<usize>,
    selected: Option<usize>,
    draft: Option<Draft>,
    todo_draft: Option<TodoDraft>,
    history_limit: usize,
    component_limit: usize,
    list_scroll: UniformListScrollHandle,
    detail_scroll: ScrollHandle,
    _search_events: Subscription,
    _type_events: Subscription,
}

impl Dashboard {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search name, stack, tags or version")
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(5)
                .placeholder("What is this project for?")
        });
        let version = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. 1.2.0-rc.1")
                .validate(|text, _| text.chars().count() <= VERSION_LIMIT)
        });
        let type_select = cx.new(|cx| {
            let groups = ProjectFilter::GROUPS
                .into_iter()
                .map(|(title, filters)| SelectGroup::new(title).items(filters.iter().copied()))
                .collect::<Vec<_>>();
            SelectState::new(
                TypeOptions::new(groups),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let type_events = cx.subscribe(&type_select, |this, _, event, cx| {
            let SelectEvent::Confirm(filter) = event;
            this.project_filter = filter.unwrap_or_default();
            this.rebuild_filter(true);
            cx.notify();
        });
        let search_events = cx.subscribe(&search, |this, entity, event, cx| match event {
            InputEvent::Change => {
                this.query = entity.read(cx).value().to_string();
                this.rebuild_filter(true);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(&index) = this.visible.first() {
                    this.select(index, cx);
                }
            }
            _ => {}
        });

        // Prevent exiting between an explicit Save and atomic replacement. The UI never waits on disk I/O.
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let Some(view) = weak.upgrade() else {
                return true;
            };
            !view.read(cx).saving
        });
        let load = cx.background_executor().spawn(async {
            let store = Store::for_user()?;
            let data = store.load();
            let catalog = data
                .as_ref()
                .ok()
                .and_then(Library::snapshot)
                .map(Catalog::new);
            Ok::<_, anyhow::Error>((store, data, catalog))
        });
        cx.spawn(async move |view, cx| {
            let result = load.await;
            let _ = view.update(cx, |this, cx| {
                this.booting = false;
                match result {
                    Ok((store, data, catalog)) => {
                        this.store = Some(store);
                        match data {
                            Ok(data) => {
                                this.allow_save = true;
                                this.root = data.last_root.clone();
                                this.library = data;
                                this.cached = catalog.is_some();
                                this.catalog = catalog;
                                this.rebuild_filter(false);
                                if let Some(root) = this.root.clone() { this.start_scan(root, cx); }
                            }
                            Err(error) => this.storage_error = Some(format!("Cannot read local data; saving is disabled for this session. {error:#}")),
                        }
                    }
                    Err(error) => this.storage_error = Some(format!("Local data storage is unavailable. {error:#}")),
                }
                cx.notify();
            });
        }).detach();

        Self {
            library: Library::default(),
            store: None,
            allow_save: false,
            dirty: false,
            saving: false,
            storage_error: None,
            root: None,
            catalog: None,
            pending_catalog: None,
            cached: false,
            booting: true,
            picking: false,
            active: None,
            scan_error: None,
            info: None,
            show_statistics: false,
            favorites_only: false,
            project_filter: ProjectFilter::default(),
            type_select,
            query: String::new(),
            search,
            description,
            version,
            visible: Vec::new(),
            selected: None,
            draft: None,
            todo_draft: None,
            history_limit: HISTORY_PAGE_SIZE,
            component_limit: COMPONENTS_PER_PAGE,
            list_scroll: UniformListScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            _search_events: search_events,
            _type_events: type_events,
        }
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        if self.booting || self.picking || self.active.is_some() || self.draft.is_some() {
            return;
        }
        self.picking = true;
        cx.notify();
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a workspace folder".into()),
        });
        cx.spawn(async move |view, cx| {
            let selection = paths.await;
            let _ = view.update(cx, |this, cx| {
                this.picking = false;
                match selection {
                    Ok(Ok(Some(paths))) => {
                        if let Some(root) = paths.into_iter().next() {
                            this.start_scan(root, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        this.scan_error =
                            Some(format!("Could not open the folder picker: {error:#}"))
                    }
                    Err(_) => {
                        this.scan_error =
                            Some("The folder picker closed without a response.".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_scan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.active.is_some() || self.draft.is_some() {
            return;
        }
        if self.root.as_ref() != Some(&root) {
            self.catalog = None;
            self.cached = false;
            self.visible.clear();
            self.selected = None;
            self.list_scroll = UniformListScrollHandle::new();
            self.reset_details();
            self.library.last_root = Some(root.clone());
            self.queue_save(cx);
        }
        self.root = Some(root.clone());
        self.scan_error = None;
        self.info = None;
        let control = Arc::new(ScanControl::default());
        self.active = Some(ActiveScan(control.clone()));
        let worker_control = control.clone();
        let worker = cx.background_executor().spawn(async move {
            scan(&root, &worker_control)
                .map(|value| value.map(|report| Catalog::new(Arc::new(report))))
        });
        let executor = cx.background_executor().clone();
        cx.spawn(async move |view, cx| {
            let mut previous = None;
            loop {
                executor.timer(Duration::from_millis(180)).await;
                let progress = control.progress();
                let Ok(running) = view.update(cx, |this, cx| {
                    let current = this
                        .active
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(&active.0, &control));
                    if current && previous != Some(progress) {
                        cx.notify();
                    }
                    current
                }) else {
                    break;
                };
                if !running {
                    break;
                }
                previous = Some(progress);
            }
        })
        .detach();
        cx.spawn(async move |view, cx| {
            let result = worker.await;
            let _ = view.update(cx, |this, cx| {
                let cancelled = this
                    .active
                    .take()
                    .is_some_and(|active| active.0.is_cancelled());
                match result {
                    _ if cancelled => this.info = Some("Scan cancelled.".into()),
                    Ok(None) => this.info = Some("Scan cancelled.".into()),
                    Err(error) => {
                        this.scan_error = Some(format!("Could not scan the workspace: {error:#}"))
                    }
                    Ok(Some(catalog)) if this.draft.is_some() => {
                        this.pending_catalog = Some(catalog)
                    }
                    Ok(Some(catalog)) => this.accept_catalog(catalog, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn accept_catalog(&mut self, catalog: Catalog, cx: &mut Context<Self>) {
        let previous = self.selected_project().map(|project| project.path.clone());
        self.root = Some(catalog.report.root.clone());
        self.library.record_scan(catalog.report.clone());
        self.selected = previous.as_deref().and_then(|path| catalog.position(path));
        self.catalog = Some(catalog);
        self.cached = false;
        self.rebuild_filter(false);
        self.queue_save(cx);
    }

    fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.active {
            active.0.cancel();
            cx.notify();
        }
    }

    fn selected_project(&self) -> Option<&Project> {
        self.catalog.as_ref()?.project(self.selected?)
    }

    fn reset_details(&mut self) {
        self.detail_scroll.set_offset(point(px(0.), px(0.)));
        self.component_limit = COMPONENTS_PER_PAGE;
        self.history_limit = HISTORY_PAGE_SIZE;
        self.todo_draft = None;
    }

    fn rebuild_filter(&mut self, reset_scroll: bool) {
        self.visible = self.catalog.as_ref().map_or_else(Vec::new, |catalog| {
            catalog.filter(
                &self.query,
                &self.library,
                self.favorites_only,
                self.project_filter,
            )
        });
        if self.draft.is_none()
            && !self
                .selected
                .is_some_and(|index| self.visible.binary_search(&index).is_ok())
        {
            self.selected = self.visible.first().copied();
            self.reset_details();
        }
        if reset_scroll && !self.visible.is_empty() {
            self.list_scroll
                .scroll_to_item_strict(0, ScrollStrategy::Top);
        }
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.draft.is_none()
            && (self.selected != Some(index) || self.show_statistics)
            && self
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.project(index))
                .is_some()
        {
            self.selected = Some(index);
            self.show_statistics = false;
            self.reset_details();
            cx.notify();
        }
    }

    fn toggle_favorites_filter(&mut self, cx: &mut Context<Self>) {
        if self.draft.is_none() && self.catalog.is_some() {
            self.favorites_only = !self.favorites_only;
            self.rebuild_filter(true);
            cx.notify();
        }
    }

    fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        if !self.allow_save || self.picking || self.draft.is_some() {
            return;
        }
        let Some(path) = self.selected_project().map(|project| project.path.clone()) else {
            return;
        };
        let favorite = !self.library.is_favorite(&path);
        if self.library.set_favorite(&path, favorite) {
            self.queue_save(cx);
            if self.favorites_only {
                self.rebuild_filter(true);
            }
            cx.notify();
        }
    }

    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.allow_save || self.picking {
            return;
        }
        let Some(project) = self.selected_project().map(|project| project.path.clone()) else {
            return;
        };
        let saved = self.library.annotation(&project);
        let description = saved
            .map(|entry| entry.description.clone())
            .unwrap_or_default();
        let tags = saved.map(|entry| entry.tags.clone()).unwrap_or_default();
        let version = saved.map(|entry| entry.version.clone()).unwrap_or_default();
        self.description
            .update(cx, |input, cx| input.set_value(description, window, cx));
        self.version
            .update(cx, |input, cx| input.set_value(version, window, cx));
        self.draft = Some(Draft { project, tags });
        self.reset_details();
        self.info = None;
        cx.notify();
    }

    fn finish_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(catalog) = self.pending_catalog.take() {
            self.accept_catalog(catalog, cx);
        } else {
            self.rebuild_filter(false);
        }
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.info = None;
        self.finish_edit(cx);
    }

    fn toggle_tag(&mut self, tag: Tag, cx: &mut Context<Self>) {
        if let Some(draft) = &mut self.draft {
            if !draft.tags.remove(&tag) {
                draft.tags.insert(tag);
            }
            cx.notify();
        }
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        if !self.allow_save {
            return;
        }
        let description = self.description.read(cx).value();
        if description.chars().count() > DESCRIPTION_LIMIT {
            self.info = Some(format!(
                "Descriptions are limited to {DESCRIPTION_LIMIT} characters."
            ));
            cx.notify();
            return;
        }
        if let Some(draft) = self.draft.take() {
            let version = self.version.read(cx).value();
            if self.library.set_annotation(
                &draft.project,
                description.trim().to_owned(),
                draft.tags,
                &version,
            ) {
                if self.pending_catalog.is_none() {
                    self.queue_save(cx);
                }
                self.info = None;
            } else {
                self.info = Some("No annotation changes to save.".into());
            }
            self.finish_edit(cx);
        }
    }

    fn can_edit_todos(&self, path: &Path) -> bool {
        self.allow_save
            && !self.picking
            && self.draft.is_none()
            && self
                .selected_project()
                .is_some_and(|project| project.path == path)
    }

    fn open_todo_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.selected_project().map(|project| project.path.clone()) else {
            return;
        };
        if !self.can_edit_todos(&project) || self.todo_draft.is_some() {
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("What needs doing?")
                .validate(|text, _| text.chars().count() <= TODO_LIMIT)
        });
        let events = cx.subscribe_in(&input, window, |this, emitter, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. })
                && this
                    .todo_draft
                    .as_ref()
                    .is_some_and(|draft| draft.input == *emitter)
            {
                this.save_todo(window, cx);
            }
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.todo_draft = Some(TodoDraft {
            project,
            input,
            _events: events,
        });
        cx.notify();
    }

    fn close_todo_draft(&mut self, cx: &mut Context<Self>) {
        self.todo_draft = None;
        cx.notify();
    }

    fn save_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.todo_draft.as_ref() else {
            return;
        };
        if !self.can_edit_todos(&draft.project) {
            return;
        }
        let title = draft.input.read(cx).value();
        if self.library.add_todo(&draft.project, &title) {
            draft.input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            });
            self.queue_save(cx);
            cx.notify();
        }
    }

    fn set_todo_completed(
        &mut self,
        path: &Path,
        id: u64,
        completed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.can_edit_todos(path) && self.library.set_todo_completed(path, id, completed) {
            self.queue_save(cx);
            cx.notify();
        }
    }

    fn remove_todo(&mut self, path: &Path, id: u64, cx: &mut Context<Self>) {
        if self.can_edit_todos(path) && self.library.remove_todo(path, id) {
            self.queue_save(cx);
            cx.notify();
        }
    }

    fn queue_save(&mut self, cx: &mut Context<Self>) {
        if !self.allow_save {
            return;
        }
        self.dirty = true;
        self.flush(cx);
    }

    fn flush(&mut self, cx: &mut Context<Self>) {
        if self.saving || !self.dirty {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        self.saving = true;
        self.dirty = false;
        let data = self.library.clone();
        let task = cx
            .background_executor()
            .spawn(async move { store.save(&data) });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        this.storage_error = None;
                        this.flush(cx);
                    }
                    Err(error) => {
                        this.dirty = true;
                        this.storage_error =
                            Some(format!("Local changes could not be saved. {error:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn progress_label(&self) -> Option<String> {
        if self.pending_catalog.is_some() {
            return Some("Scan complete · finish editing to apply the updated results".into());
        }
        let active = self.active.as_ref()?;
        if active.0.is_cancelled() {
            return Some("Cancelling scan…".into());
        }
        let progress = active.0.progress();
        Some(match progress.phase {
            Phase::Walking => format!(
                "Searching folders · {} visited",
                progress.visited_directories
            ),
            Phase::Classifying => format!(
                "Detecting stacks · {} / {} candidate folders",
                progress.classified_directories, progress.candidate_directories
            ),
            Phase::Organizing => "Grouping projects and reading local Git metadata…".into(),
        })
    }
}
