"use strict";

const ideaBoards = (() => {
  const statuses = {raw: "Raw", exploring: "Exploring", parked: "Parked", promoted: "Promoted"};
  const modes = {write: "Write", split: "Split", preview: "Preview"};
  const scratchDrafts = new Map(), ideaDrafts = new Map(), requests = new Map();
  const newIdea = "new";
  let nextToken = 0, scratch = null, activeIdea = null, selectedIdea = null;
  let scratchEditor, ideaEditor, form, knownProjects, projectOptions;
  let projectsById = new Map(), ideaFilter = "", confirmDelete = false;
  const request = (op, args = {}) => new Promise((resolve, reject) => {
    const token = ++nextToken;
    requests.set(token, {resolve, reject});
    post("board", {request: {token, op, ...args}});
  });
  window.receiveBoard = ({token, value, error}) => {
    const waiting = requests.get(token);
    if (!waiting) return;
    requests.delete(token);
    if (error) waiting.reject(error);
    else waiting.resolve(value);
  };

  function markdownEditor(host, label, onInput) {
    host.innerHTML = `<div class="row markdown-toolbar" role="group" aria-label="Markdown editor mode">
      ${Object.entries(modes).map(([key, name]) => `<button class="btn" type="button" data-mode="${key}" aria-pressed="false">${name}</button>`).join("")}
      <span class="spacer"></span><span class="ch">Markdown</span>
    </div><div class="markdown-panes">
      <textarea class="in mono" rows="7" aria-label="${label}" placeholder="# Start with an idea"></textarea>
      <div class="markdown-preview" role="region" aria-label="Markdown preview" hidden></div>
    </div>`;
    const input = host.querySelector("textarea"), panes = input.parentElement, preview = input.nextElementSibling;
    let model = null, mode = "write", timer, generation = 0, wanted, rendered;
    function refresh() {
      const markdown = model && mode !== "write" ? model.markdown : undefined;
      if (markdown === wanted) return;
      wanted = markdown;
      clearTimeout(timer);
      const current = ++generation;
      if (markdown === undefined) return;
      if (!markdown.trim()) {
        setContent(preview, '<p class="t3">Nothing to preview yet.</p>');
        rendered = undefined;
      } else if (markdown !== rendered) {
        timer = setTimeout(async () => {
          try {
            const html = await request("preview", {markdown});
            if (current === generation) {
              setContent(preview, html);
              rendered = markdown;
            }
          } catch {
            if (current === generation) {
              setContent(preview, '<p class="t3">Preview unavailable.</p>');
              rendered = undefined;
            }
          }
        }, 110);
      }
    }
    function updateMode() {
      for (const button of host.querySelectorAll("[data-mode]")) {
        button.setAttribute("aria-pressed", String(button.dataset.mode === mode));
      }
      input.hidden = mode === "preview";
      preview.hidden = mode === "write";
      panes.classList.toggle("split", mode === "split");
      refresh();
    }
    host.addEventListener("click", ({target}) => {
      if (!target.dataset.mode) return;
      mode = target.dataset.mode;
      updateMode();
    });
    input.addEventListener("input", () => {
      if (!model) return;
      model.markdown = input.value;
      onInput(model);
      refresh();
    });
    updateMode();
    return {
      bind(next, editable) {
        const markdown = next?.markdown ?? "";
        if (input.value !== markdown) input.value = markdown;
        input.disabled = !editable;
        const limit = workspace.markdown_limit ?? 32000;
        if (input.maxLength !== limit) input.maxLength = limit;
        if (wanted !== undefined && next !== model) {
          setContent(preview, '<p class="t3">Rendering preview…</p>');
          rendered = wanted = undefined;
          clearTimeout(timer);
          generation++;
        }
        model = next;
        refresh();
      },
    };
  }

  const alteredScratch = draft => draft.markdown !== draft.saved;
  const saveStatus = (draft, dirty) => draft.error || (draft.loading ? "Loading…" : draft.pending ? "Saving…"
    : dirty ? "Unsaved changes" : workspace.storage_error && workspace.editable ? "Save failed · use Retry save"
    : workspace.saving ? "Saving…" : "Saved locally");

  function scratchControls() {
    if (!scratch) return;
    const dirty = alteredScratch(scratch);
    $("scratch-save").disabled = !workspace.editable || scratch.loading || scratch.unavailable || scratch.pending || !dirty;
    $("scratch-state").textContent = saveStatus(scratch, dirty);
  }

  function showScratchpad(project) {
    if (scratch?.id !== project?.id) {
      if (scratch && !alteredScratch(scratch) && !scratch.pending) scratchDrafts.delete(scratch.id);
      scratch = null;
      if (project) {
        let draft = scratchDrafts.get(project.id);
        if (!draft) {
          draft = {id: project.id, markdown: "", saved: "", loading: project.has_scratchpad};
          scratchDrafts.set(project.id, draft);
          if (draft.loading) request("read_scratchpad", {id: draft.id}).then(value => {
            draft.markdown = draft.saved = value;
          }).catch(error => { draft.error = error; draft.unavailable = true; }).finally(() => {
            draft.loading = false;
            if (scratch === draft) showScratchpad(project);
          });
        }
        scratch = draft;
      }
    }
    scratchEditor.bind(scratch, workspace.editable && !scratch?.loading && !scratch?.unavailable);
    scratchControls();
  }

  async function saveScratchpad() {
    const draft = scratch;
    if (!draft || $("scratch-save").disabled) return;
    const markdown = draft.markdown;
    draft.pending = true;
    draft.error = "";
    scratchControls();
    try {
      draft.saved = await request("save_scratchpad", {id: draft.id, markdown});
      if (draft.markdown === markdown) draft.markdown = draft.saved;
    } catch (error) { draft.error = error; }
    finally {
      draft.pending = false;
      if (scratch === draft) {
        scratchEditor.bind(draft, workspace.editable);
        scratchControls();
      } else if (!alteredScratch(draft)) scratchDrafts.delete(draft.id);
    }
  }

  const fieldsOf = idea => ({title: idea.title, markdown: idea.markdown, status: idea.status,
    tags: idea.tags, project_id: idea.project_id});
  const makeDraft = () => ({title: "", markdown: "", status: "raw", tags: [], project_id: null});
  const tagText = tags => tags.join(", ");
  const dirtyIdea = draft => draft.loaded && (draft.title !== draft.saved.title || draft.markdown !== draft.saved.markdown
    || draft.status !== draft.saved.status || draft.tagsText !== tagText(draft.saved.tags) || draft.project_id !== draft.saved.project_id);
  function forgetClean(draft) {
    if (draft && !dirtyIdea(draft) && !draft.pending && !draft.loading) ideaDrafts.delete(draft.key);
  }
  function loadDraft(key) {
    if (ideaDrafts.has(key)) return ideaDrafts.get(key);
    const blank = makeDraft();
    const draft = {key, ...blank, saved: blank, tagsText: "", loaded: key === newIdea};
    ideaDrafts.set(key, draft);
    if (!draft.loaded) {
      draft.loading = true;
      request("read_idea", {id: key}).then(idea => {
        const saved = fieldsOf(idea);
        Object.assign(draft, saved, {saved, loaded: true, tagsText: tagText(saved.tags)});
      }).catch(error => { draft.error = error; }).finally(() => {
        draft.loading = false;
        if (draft === activeIdea && page === "ideas") renderIdeaEditor(true);
        else if (draft !== activeIdea) forgetClean(draft);
      });
    }
    return draft;
  }

  function renderIdeas() {
    const ideas = workspace.ideas ?? [];
    if (knownProjects !== workspace.projects) {
      knownProjects = workspace.projects;
      projectsById = new Map(workspace.projects.map(project => [project.id, project]));
      projectOptions = undefined;
    }
    const terms = $("idea-search").value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const counts = {raw: 0, exploring: 0, parked: 0, promoted: 0};
    const matching = [];
    for (const idea of ideas) {
      counts[idea.status]++;
      if (ideaFilter && idea.status !== ideaFilter) continue;
      const linked = projectsById.get(idea.project_id)?.name;
      if (terms.length) {
        const haystack = [idea.title, idea.excerpt, ...idea.tags, linked].join("\n").toLowerCase();
        if (!terms.every(term => haystack.includes(term))) continue;
      }
      matching.push(idea);
    }
    matching.sort((a, b) => b.updated_at.localeCompare(a.updated_at) || b.id - a.id);
    // Filtering hides list entries, not an editor with changes in progress.
    if (selectedIdea !== newIdea && !ideas.some(idea => idea.id === selectedIdea)) selectedIdea = matching[0]?.id ?? null;
    const previous = activeIdea;
    activeIdea = selectedIdea == null ? null : loadDraft(selectedIdea);
    if (previous !== activeIdea) { forgetClean(previous); confirmDelete = false; }
    setContent($("idea-filters"), [["", "All", ideas.length], ...Object.entries(statuses).map(([key, label]) => [key, label, counts[key]])]
      .map(([key, label, count]) => `<button class="btn" data-idea-filter="${key}" data-focus="ideas:${key}" aria-pressed="${ideaFilter === key}">${label} <span class="t3">${count}</span></button>`).join(""));
    setContent($("idea-results"), matching.map(idea => `<button class="idea-row" data-idea="${idea.id}" data-focus="idea:${idea.id}"${idea.id === selectedIdea ? ' aria-current="true"' : ""}>
      <b class="ellipsis">${text(idea.title)}</b><span class="t3 ellipsis">${text(idea.excerpt || "No Markdown yet")}</span>
      <span class="row idea-chips">${chips([statuses[idea.status], ...idea.tags.slice(0, 3)])}</span></button>`).join("")
      || `<div class="empty t3">${workspace.loading ? "Loading ideas…" : ideas.length ? "No ideas match these filters." : "No ideas yet. Create one to start writing."}</div>`);
    renderIdeaEditor(previous !== activeIdea);
  }

  function renderIdeaEditor(force) {
    const draft = activeIdea;
    form.hidden = !draft?.loaded;
    $("idea-empty").hidden = !form.hidden;
    if (!draft?.loaded) {
      $("idea-empty").textContent = draft?.error || (draft?.loading ? "Loading idea…" : "Select an idea or create a new one.");
      ideaEditor.bind(null, false);
      return;
    }
    const editable = workspace.editable;
    if (force) {
      for (const [id, value] of [["idea-title", draft.title], ["idea-status", draft.status], ["idea-tags", draft.tagsText]]) {
        if ($(id).value !== value) $(id).value = value;
      }
    }
    $("idea-title").maxLength = workspace.idea_title_limit ?? 120;
    for (const id of ["idea-title", "idea-status", "idea-tags", "idea-project"]) $(id).disabled = !editable;
    ideaEditor.bind(draft, editable);
    renderLink();
    ideaControls();
  }

  function renderLink() {
    const draft = activeIdea;
    const promoted = draft.status === "promoted";
    $("idea-linked").hidden = !promoted;
    if (!promoted) return;
    const linked = projectsById.get(draft.project_id);
    const missing = draft.project_id && !linked;
    projectOptions ??= [...projectsById.values()].sort(orderProjects).map(project =>
      `<option value="${text(attrId(project.id))}">${text(project.name)} · ${text(shortPath(project.path, workspace.home, workspace.windows))}</option>`).join("");
    setContent($("idea-project"), '<option value="">Choose a scanned project…</option>'
      + (missing ? `<option value="${text(attrId(draft.project_id))}">Linked project is not in the current scan</option>` : "") + projectOptions);
    $("idea-project").value = draft.project_id ? attrId(draft.project_id) : "";
    $("idea-view-project").disabled = !linked;
  }

  function ideaControls() {
    const draft = activeIdea;
    if (!draft?.loaded) return;
    const dirty = dirtyIdea(draft);
    $("idea-save").disabled = !workspace.editable || draft.pending || !draft.title.trim()
      || (draft.status === "promoted" && !draft.project_id) || (draft.key !== newIdea && !dirty);
    $("idea-discard").hidden = draft.key !== newIdea && !dirty;
    $("idea-discard").disabled = draft.pending;
    $("idea-delete").hidden = draft.key === newIdea || !workspace.editable;
    $("idea-delete").textContent = confirmDelete ? "Confirm delete" : "Delete";
    $("idea-delete").disabled = draft.pending;
    $("idea-state").textContent = draft.status === "promoted" && !draft.project_id && !draft.pending
      ? "Choose a scanned project to promote this idea." : draft.key === newIdea && !dirty ? "New draft" : saveStatus(draft, dirty);
  }

  async function saveIdea() {
    const draft = activeIdea;
    if (!draft || $("idea-save").disabled) return;
    const fields = fieldsOf(draft), tagsText = draft.tagsText;
    fields.tags = tagsText.split(",").map(tag => tag.trim()).filter(Boolean);
    draft.pending = true;
    draft.error = "";
    confirmDelete = false;
    ideaControls();
    try {
      const idea = await request("save_idea", {id: draft.key === newIdea ? null : draft.key, draft: fields});
      const saved = fieldsOf(idea);
      const unchanged = tagText(fields.tags) === tagText(draft.tagsText.split(",").map(tag => tag.trim()).filter(Boolean))
        && ["title", "markdown", "status", "project_id"].every(field => draft[field] === fields[field]);
      if (draft.key === newIdea) {
        ideaDrafts.delete(newIdea);
        if (selectedIdea === newIdea) {
          selectedIdea = idea.id;
          ideaFilter = "";
          $("idea-search").value = "";
        }
        draft.key = idea.id;
        ideaDrafts.set(idea.id, draft);
      }
      draft.saved = saved;
      if (unchanged) Object.assign(draft, saved, {tagsText: tagText(saved.tags)});
    } catch (error) { draft.error = error; }
    finally {
      draft.pending = false;
      if (page === "ideas") { renderIdeas(); if (draft === activeIdea) renderIdeaEditor(true); }
      if (draft !== activeIdea) forgetClean(draft);
    }
  }

  async function removeIdea() {
    const draft = activeIdea;
    if (!draft || draft.pending || draft.key === newIdea) return;
    if (!confirmDelete) { confirmDelete = true; ideaControls(); return; }
    confirmDelete = false;
    draft.pending = true;
    ideaControls();
    try {
      await request("remove_idea", {id: draft.key});
      ideaDrafts.delete(draft.key);
    } catch (error) { draft.error = error; }
    finally {
      draft.pending = false;
      if (page === "ideas") renderIdeas();
    }
  }

  function init() {
    scratchEditor = markdownEditor($("scratch-markdown"), "Project scratchpad, Markdown", draft => {
      draft.error = ""; scratchControls();
    });
    ideaEditor = markdownEditor($("idea-markdown"), "Idea body, Markdown", draft => {
      draft.error = ""; confirmDelete = false; ideaControls();
    });
    $("scratch-save").addEventListener("click", saveScratchpad);
    $("scratchpad").addEventListener("keydown", event => {
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); saveScratchpad(); }
    });
    $("idea-search").addEventListener("input", renderIdeas);
    $("idea-results").addEventListener("click", ({target}) => {
      const button = target.closest("[data-idea]");
      if (button) { selectedIdea = Number(button.dataset.idea); renderIdeas(); }
    });
    $("idea-filters").addEventListener("click", ({target}) => {
      const button = target.closest("[data-idea-filter]");
      if (button) { ideaFilter = button.dataset.ideaFilter; renderIdeas(); }
    });
    form = $("idea-form");
    form.addEventListener("submit", event => { event.preventDefault(); saveIdea(); });
    form.addEventListener("keydown", event => {
      if (event.key === "Escape" && confirmDelete) { confirmDelete = false; ideaControls(); }
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); saveIdea(); }
    });
    for (const [id, field] of [["idea-title", "title"], ["idea-tags", "tagsText"], ["idea-status", "status"], ["idea-project", "project_id"]]) {
      $(id).addEventListener(id === "idea-status" || id === "idea-project" ? "change" : "input", ({target}) => {
        activeIdea[field] = field === "project_id" ? (target.value ? decodeURIComponent(target.value) : null) : target.value;
        activeIdea.error = "";
        confirmDelete = false;
        if (field === "status" && target.value !== "promoted") activeIdea.project_id = null;
        if (field === "status" || field === "project_id") renderLink();
        ideaControls();
      });
    }
    $("idea-discard").addEventListener("click", () => {
      const {key, saved} = activeIdea;
      if (key === newIdea) {
        ideaDrafts.delete(newIdea);
        selectedIdea = activeIdea = null;
        renderIdeas();
      } else {
        Object.assign(activeIdea, saved, {tagsText: tagText(saved.tags), error: ""});
        confirmDelete = false;
        renderIdeaEditor(true);
      }
    });
    $("idea-delete").addEventListener("click", removeIdea);
    $("idea-view-project").addEventListener("click", () => openLinkedProject(activeIdea.project_id));
  }
  function createIdea() {
    if (!workspace.editable) return;
    selectedIdea = newIdea;
    renderIdeas();
    $("idea-title").focus();
  }
  return {init, render: renderIdeas, createIdea, showScratchpad};
})();
