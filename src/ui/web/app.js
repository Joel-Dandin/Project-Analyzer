"use strict";

const $ = id => document.getElementById(id);
const post = (action, args = {}) => window.ipc.postMessage(JSON.stringify({action, ...args}));
const content = new WeakMap();
const setContent = (element, html) => {
  if (content.get(element) === html) return;
  const focus = element.contains(document.activeElement) ? document.activeElement.dataset.focus : null;
  element.innerHTML = html;
  content.set(element, html);
  if (focus) {
    [...element.querySelectorAll("[data-focus]")].find(node => node.dataset.focus === focus)?.focus({preventScroll: true});
  }
};
const disabled = value => value ? " disabled" : "";
const attrId = value => encodeURIComponent(value);
const starPath = '<path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z" stroke="#6B6F76" stroke-width="1.8" stroke-linejoin="round"></path>';
const lockIcon = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#80838A" stroke-width="2" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"></rect><path d="M8 11V8a4 4 0 0 1 8 0v3"></path></svg>';
let workspace = {projects: [], folders: [], history: [], loading: true};
let page = "projects", kind = "", query = "", starred = false, grouped = true, showDetails = true;
let selected = null, editorId = null, menu = null, anchor = null;
let metrics = summarizeWorkspace(workspace);

function filtered(projects) {
  const version = $("version-search").value.trim().toLowerCase();
  return projects.filter(project => matchesProject(project, {kind, starred, query, version}));
}

function nav(label, action, value, count, active) {
  return `<button class="nav${active ? " on" : ""}" data-action="${action}" data-value="${value}" data-focus="${value}"${active ? ' aria-current="true"' : ""}>
    <span class="sq" aria-hidden="true"></span><span class="ellipsis">${text(label)}</span>${count == null ? "" : `<b>${count}</b>`}
  </button>`;
}

function renderSidebar() {
  const {folders, projects, loading, progress, home, windows} = workspace;
  const busy = Boolean(progress);
  $("start").textContent = busy ? (progress.cancelled ? "Cancelling…" : "Cancel scan") : "Start scan";
  $("start").disabled = loading || Boolean(progress?.cancelled);
  $("rescan").disabled = loading || busy || !folders.length;
  $("choose").disabled = loading || busy;
  const all = page === "projects";
  setContent($("workspace-nav"), nav("All projects", "page", "projects", projects.length, all && !starred && !kind)
    + nav("Starred", "page", "starred", metrics.starred, all && starred)
    + nav("Idea board", "page", "ideas", workspace.ideas?.length ?? 0, page === "ideas")
    + nav("Statistics", "page", "statistics", null, page === "statistics"));
  setContent($("folders"), folders.map(folder => `<div class="folder row">
    <span class="ellipsis" title="${text(folder.path)}">${text(shortPath(folder.path, home, windows))}</span>
    <button class="x" data-action="remove-folder" data-value="${text(attrId(folder.id))}" aria-label="Remove ${text(folder.path)}"${disabled(busy || loading)}>×</button>
  </div>`).join(""));
  const types = Object.entries(projectTypes);
  setContent($("type-nav"), types.map(([key, label], index) => {
    const count = projects.filter(project => matchesType(project, key)).length;
    return index < 8 || count || key === kind ? nav(label, "kind", key, count, all && key === kind) : "";
  }).join(""));
  const counts = new Map(metrics.project_types.map(({label, count}) => [label, count]));
  $("total").textContent = projects.length;
  setContent($("total-bar"), types.map(([, label], index) => counts.has(label)
    ? `<i title="${text(label)}" style="flex:${counts.get(label)};background:rgba(255,255,255,${(0.26 - index * 0.01).toFixed(2)})"></i>` : "").join(""));
}

function renderHeader(count) {
  const {loading, progress, history, cached, folders, notice, storage_error} = workspace;
  const stats = page === "statistics";
  const ideas = page === "ideas";
  $("page-heading").textContent = stats ? "Statistics" : ideas ? "Idea board" : starred ? "Starred" : "All projects";
  $("page-count").textContent = count;
  $("header-action").textContent = stats ? "Export" : ideas ? "+ New idea" : "Rescan";
  $("header-action").classList.toggle("pri", ideas);
  $("header-action").disabled = loading || (ideas ? !workspace.editable : !stats && (Boolean(progress) || !folders.length));
  $("toolbar").hidden = page !== "projects";
  $("project-view").hidden = page !== "projects";
  $("idea-board").hidden = !ideas;
  $("statistics").hidden = !stats;
  $("progress").hidden = !progress && !loading;
  setContent($("progress"), progress ? `<i class="dot"></i>${progress.cancelled ? "Cancelling scan…" : `Scanning · ${progress.done} of ${progress.total} folders`}` : "Loading workspace…");
  $("notice").hidden = !(notice || storage_error);
  $("notice-text").textContent = [storage_error, notice].filter(Boolean).join(" · ");
  $("retry").hidden = !storage_error || !workspace.editable;
  $("starred-filter").setAttribute("aria-pressed", String(starred));
  $("filter-button").classList.toggle("selected-filter", Boolean(kind || query));
  const last = history.at(-1);
  $("cache").textContent = cached ? (last ? `${progress ? "Cached" : "Scanned"} ${timeAgo(last.at)}${progress ? " · refreshing" : ""}` : "Cached scan") : "";
}

function star(project, inDetails = false) {
  const name = project.starred ? "Unstar" : "Star";
  const id = text(attrId(project.id));
  return `<button class="ib" data-action="star" data-value="${id}" data-focus="star:${id}" aria-label="${name}${inDetails ? " project" : ` ${text(project.name)}`}" aria-pressed="${project.starred}"${disabled(!workspace.editable)}>
    <svg width="13" height="13" viewBox="0 0 24 24" fill="${project.starred ? "#8A8D94" : "none"}" aria-hidden="true">${starPath}</svg>
  </button>`;
}

function chips(values) {
  return values.map(value => `<span class="ch ellipsis" title="${text(value)}">${text(value)}</span>`).join("");
}

function remoteName(remote) {
  return [remote.host, remote.account, remote.repository].filter(Boolean).join(" / ");
}

function renderList(projects) {
  const groups = new Map(Object.keys(projectTypes).map(key => [key, []]));
  const sorted = [...projects].sort(orderProjects);
  for (const project of sorted) groups.get(project.kind).push(project);
  const row = project => {
    const id = text(attrId(project.id));
    const remote = primaryRemote(project);
    const path = shortPath(project.path, workspace.home, workspace.windows);
    const meta = `${path}${remote ? ` · ${[remote.host, remote.account].filter(Boolean).join("/")}` : ""}`;
    return `<div class="r${project.id === selected ? " sel" : ""}">
      ${star(project)}${statusDot(project.statuses)}
      <button class="project-select" data-action="select" data-value="${id}" data-focus="select:${id}"${project.id === selected ? ' aria-current="true"' : ""}>
        <span class="n"><span class="ellipsis" title="${text(project.name)}">${text(project.name)}</span>${chips(project.stack.slice(0, 2))}</span>
        <span class="s mono ellipsis" title="${text(meta)}">${text(meta)}</span>
      </button>
      <span class="row row-tags">${chips(project.tags.slice(0, 2))}</span>
      <span class="t3 mono row-version ellipsis" title="${text(project.versions.join(", "))}">${text(project.versions[0])}</span>
    </div>`;
  };
  const empty = workspace.loading ? "Loading workspace…" : !workspace.folders.length ? "Choose a folder and start a scan to find projects." : workspace.progress ? "Looking for projects…" : "No projects match this view.";
  setContent($("project-list"), !sorted.length ? `<div class="t3 empty">${empty}</div>` : grouped
    ? [...groups].filter(([, items]) => items.length).map(([key, items]) => `<div class="grp"><b>${projectTypes[key]}</b><span class="t3">${items.length}</span></div>${items.map(row).join("")}`).join("")
    : sorted.map(row).join(""));
}

function labelChips(project, field, values) {
  return values.map(value => `<span class="ch label-chip"><span class="ellipsis" title="${text(value)}">${text(value)}</span>
    <button class="x" data-action="remove-label" data-field="${field}" data-value="${text(attrId(value))}" aria-label="Remove ${field} ${text(value)}"${disabled(!workspace.editable)}>×</button></span>`).join("");
}

function renderDetails(project) {
  $("details").hidden = !showDetails;
  $("detail-empty").hidden = Boolean(project);
  $("editor").hidden = !project;
  ideaBoards.showScratchpad(showDetails ? project : null);
  if (!project) { editorId = null; return; }
  const sameProject = editorId === project.id;
  if (!sameProject) {
    $("details").scrollTop = 0;
    for (const input of $("editor").querySelectorAll("form input")) input.value = "";
    editorId = project.id;
  }
  setContent($("detail-heading"), `${star(project, true)}<h2 class="ellipsis spacer" title="${text(project.name)}">${text(project.name)}</h2><span class="ch">${projectTypes[project.kind]}</span>`);
  $("detail-path").textContent = shortPath(project.path, workspace.home, workspace.windows);
  $("detail-path").title = project.path;
  setContent($("status-value"), `${statusDot(project.statuses)}<span>${text(statusSummary(project.statuses))}</span>`);
  const remote = primaryRemote(project);
  setContent($("repo-property"), `<span>Git</span>${lockIcon}<span class="t2 git-value ellipsis" title="${text(remote ? remoteName(remote) : "")}">
    ${remote ? `<b>${text(remote.host)}</b>${[remote.account, remote.repository].filter(Boolean).map(part => ` / ${text(part)}`).join("")}` : project.repository ? "Local repository (no remote)" : "No repository"}
  </span><button class="btn compact open-project" type="button" data-action="open-repository" data-focus="open-repository"${disabled(!remote)} title="${remote ? "Open the repository in the default browser" : "No remote repository detected"}">Open in browser</button>`);
  setContent($("stack"), chips(project.stack));
  setContent($("tags"), labelChips(project, "tag", project.tags));
  setContent($("versions"), labelChips(project, "version", project.versions));
  for (const field of ["summary", "notes"]) {
    const input = $(field);
    if (!sameProject || document.activeElement !== input) input.value = project[field];
    input.disabled = !workspace.editable;
  }
  $("change-status").disabled = !workspace.editable;
  for (const input of $("editor").querySelectorAll("form input")) input.disabled = !workspace.editable;
  $("todo-count").textContent = `${project.todos.filter(todo => todo.completed).length} / ${project.todos.length}`;
  setContent($("todos"), project.todos.map(todo => `<div class="todo">
    <label class="spacer"><input type="checkbox" data-task="${todo.id}" data-focus="task:${todo.id}"${todo.completed ? " checked" : ""}${disabled(!workspace.editable)}>
    <span>${text(todo.title)}</span></label><button class="x" data-action="remove-todo" data-value="${todo.id}" aria-label="Remove task ${text(todo.title)}"${disabled(!workspace.editable)}>×</button>
  </div>`).join(""));
  setContent($("activity"), [...project.history].reverse().map(change => `<div>${statusDot(change.current, true)}Status → ${text(statusSummary(change.current))}
    <time class="t3 mono" datetime="${text(change.at)}" title="${text(statusSummary(change.previous))} → ${text(statusSummary(change.current))} · ${text(change.at)} (UTC)">${text(formatUtc(change.at))}</time></div>`).join("") || '<span class="t3">No status changes yet.</span>');
  const remotes = project.repository?.remotes ?? [];
  const sources = $("sources");
  sources.hidden = !remotes.length && !project.components.length;
  const html = `<ul class="source-list">${remotes.map(item => `<li>${text(item.name)}: ${text(remoteName(item))}${item.has_tracking_refs ? " · local tracking refs" : ""}
    ${(item.push_targets ?? []).map(target => `<div class="t3">Push: ${text(remoteName(target))}</div>`).join("")}</li>`).join("")}
    ${project.components.map(part => `<li><span class="mono">${text(shortPath(part.path, workspace.home, workspace.windows))}</span>
      <div class="t3">${text([...part.stack, ...part.signals].join(" · "))}</div></li>`).join("")}</ul>`;
  // Keep the expanded state during asynchronous updates.
  setContent(sources, `<details${sameProject && sources.firstElementChild?.open ? " open" : ""}><summary>Detected sources</summary>${html}</details>`);
}

function render() {
  renderSidebar();
  if (page !== "projects") ideaBoards.showScratchpad(null);
  if (page === "ideas") {
    renderHeader(workspace.ideas?.length ?? 0);
    ideaBoards.render();
    return;
  }
  if (page === "statistics") {
    renderHeader(workspace.projects.length);
    setContent($("statistics"), renderStatistics(metrics));
    return;
  }
  const projects = filtered(workspace.projects);
  renderHeader(projects.length);
  const project = projects.find(item => item.id === selected) ?? [...projects].sort(orderProjects)[0];
  selected = project?.id ?? null;
  renderList(projects);
  renderDetails(project);
}

window.receive = next => {
  workspace = {...next, projects: next.projects.map(project => ({...project,
    tags: project.tags ?? [], versions: project.versions ?? [], statuses: project.statuses ?? [],
    todos: project.todos ?? [], history: project.history ?? [],
  }))};
  metrics = summarizeWorkspace(workspace);
  render();
};

function openLinkedProject(id) {
  if (!workspace.projects.some(project => project.id === id)) return;
  page = "projects";
  kind = query = "";
  starred = false;
  showDetails = true;
  selected = id;
  $("version-search").value = "";
  render();
}

function edit(field, args, id = editorId) {
  if (workspace.editable && id != null) post("edit", {id, field, ...args});
}

function saveText(field) {
  const input = $(field);
  const project = workspace.projects.find(item => item.id === editorId);
  if (project && input.value !== project[field]) edit(field, {value: input.value});
}

function closeMenu(restore = false) {
  if (anchor) {
    anchor.setAttribute("aria-expanded", "false");
    if (restore) anchor.focus({preventScroll: true});
  }
  $("popover").hidden = true;
  menu = anchor = null;
}

function openMenu(name, button) {
  const previous = menu;
  closeMenu();
  if (previous === name) return;
  const popup = $("popover");
  menu = name;
  anchor = button;
  anchor.setAttribute("aria-expanded", "true");
  const project = workspace.projects.find(item => item.id === editorId);
  if (name === "filter") {
    setContent(popup, `<label class="menu-label" for="project-search">Search projects</label>
      <input class="in" id="project-search" type="search" placeholder="Name, path, stack, tags…">
      <label class="menu-label" for="kind-filter">Type</label>
      <select class="in" id="kind-filter"><option value="">All project types</option>
        ${Object.entries(filters).map(([key, label]) => `<option value="${key}">${text(label)}</option>`).join("")}</select>
      <button class="btn menu-footer" data-action="reset">Clear filters</button>`);
    $("project-search").value = query;
    $("kind-filter").value = kind;
  } else if (name === "display") {
    setContent(popup, `<label class="check-option"><input type="checkbox" id="grouped"${grouped ? " checked" : ""}>Group projects by type</label>
      <label class="check-option"><input type="checkbox" id="show-details"${showDetails ? " checked" : ""}>Show project details</label>`);
  } else if (project) {
    setContent(popup, `<form id="status-form"><div class="menu-label">Status</div>
      ${statusOrder.map(value => `<label class="check-option"><input type="checkbox" value="${value}"${project.statuses.includes(value) ? " checked" : ""}>${text(statusLabel(value))}</label>`).join("")}
      <button class="btn pri menu-footer" type="submit">Apply</button></form>`);
  }
  popup.setAttribute("aria-label", name === "status" ? "Change status" : name === "filter" ? "Filter projects" : "Display options");
  popup.hidden = false;
  const rect = anchor.getBoundingClientRect();
  popup.style.left = `${Math.max(8, Math.min(rect.left, innerWidth - popup.offsetWidth - 8))}px`;
  popup.style.top = `${Math.max(8, Math.min(rect.bottom + 5, innerHeight - popup.offsetHeight - 8))}px`;
  popup.querySelector("input")?.focus();
}

document.addEventListener("click", event => {
  const target = event.target.closest("[data-action]");
  if (menu && !$("popover").contains(event.target) && target?.dataset.action !== "popover") closeMenu();
  if (!target || target.disabled) return;
  const {action, value} = target.dataset;
  switch (action) {
    case "popover": openMenu(target.dataset.menu, target); return;
    case "start": post(workspace.progress ? "cancel" : "start"); return;
    case "rescan": post("start"); return;
    case "choose": case "retry_save": post(action); return;
    case "header":
      if (page === "statistics") post("export", {report: JSON.stringify(metrics, null, 2)});
      else if (page === "ideas") ideaBoards.createIdea();
      else post("start");
      return;
    case "remove-folder": post("remove_folder", {id: decodeURIComponent(value)}); return;
    case "page":
      page = value === "statistics" || value === "ideas" ? value : "projects";
      if (page === "projects") { kind = ""; starred = value === "starred"; }
      break;
    case "kind": page = "projects"; kind = kind === value ? "" : value; break;
    case "starred": starred = !starred; break;
    case "reset": kind = query = ""; starred = false; $("version-search").value = ""; closeMenu(true); break;
    case "select": selected = decodeURIComponent(value); showDetails = true; break;
    case "open-folder": case "open-repository":
      if (editorId != null) post("open", {id: editorId, target: action === "open-folder" ? "folder" : "repository"});
      return;
    case "star": {
      const id = decodeURIComponent(value);
      const project = workspace.projects.find(item => item.id === id);
      if (project) edit("star", {value: !project.starred}, id);
      return;
    }
    case "remove-label": edit(target.dataset.field, {value: decodeURIComponent(value), present: false}); return;
    case "remove-todo": edit("remove_todo", {task: Number(value)}); return;
  }
  render();
});

document.addEventListener("input", ({target}) => {
  if (target.id === "project-search") query = target.value.trim().toLowerCase();
  if (target.id === "project-search" || target.id === "version-search") render();
});

document.addEventListener("change", ({target}) => {
  if (target.id === "summary" || target.id === "notes") saveText(target.id);
  else if (target.dataset.task) edit("toggle_todo", {task: Number(target.dataset.task), value: target.checked});
  else if (target.id === "kind-filter") { kind = target.value; render(); }
  else if (target.id === "grouped") { grouped = target.checked; render(); }
  else if (target.id === "show-details") { showDetails = target.checked; render(); }
});

document.addEventListener("submit", event => {
  event.preventDefault();
  const form = event.target;
  if (form.id === "status-form") {
    edit("status", {value: [...form.querySelectorAll("input:checked")].map(input => input.value)});
    closeMenu(true);
  } else if (form.dataset.add) {
    const input = form.querySelector("input");
    const value = input.value.trim();
    if (value) edit(form.dataset.add, {value, present: true});
    input.value = "";
  }
});

document.addEventListener("keydown", event => {
  if (event.key === "Escape" && menu) { event.preventDefault(); closeMenu(true); }
  if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && ["summary", "notes"].includes(event.target.id)) {
    event.preventDefault();
    event.target.blur();
  }
});
window.addEventListener("resize", () => closeMenu());

ideaBoards.init();
render();
post("ready");
