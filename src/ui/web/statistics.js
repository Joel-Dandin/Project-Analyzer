"use strict";

const palette = ["#5E6AD2", "#B0B3BA", "#80838A", "#5A5D63", "#3A3C41"];
const languageNames = new Set(["C#", "Dart", "Go", "Groovy", "Java", "JavaScript", "Kotlin", "Python", "Rust", "Swift", "TypeScript"]);
const activeStatuses = ["in_progress", "developing"];

function summarizeWorkspace({projects, folders, history, last_report}) {
  const counts = {
    project_types: new Map(), languages: new Map(), frameworks: new Map(),
    statuses: new Map(), git_hosts: new Map(), owners: new Map(), tags: new Map(), versions: new Map(),
  };
  const increment = (map, key) => map.set(key, (map.get(key) ?? 0) + 1);
  let starred = 0, active = 0, labelled = 0, git = 0, remotes = 0, total = 0, done = 0, mostOpen = null;
  const changes = [];
  for (const project of projects) {
    increment(counts.project_types, projectTypes[project.kind]);
    increment(counts.statuses, statusLabel(primaryStatus(project.statuses)));
    for (const item of new Set(project.stack)) increment(languageNames.has(item) ? counts.languages : counts.frameworks, item);
    for (const tag of new Set(project.tags)) increment(counts.tags, tag);
    for (const version of new Set(project.versions)) increment(counts.versions, version);
    starred += Number(project.starred);
    active += Number(project.statuses.some(value => activeStatuses.includes(value)));
    labelled += Number(project.versions.length > 0);
    git += Number(Boolean(project.repository));
    const remote = primaryRemote(project);
    remotes += Number(Boolean(remote));
    increment(counts.git_hosts, remote?.host ?? "No remote");
    if (remote?.account) increment(counts.owners, remote.account);
    const completed = project.todos.filter(todo => todo.completed).length;
    total += project.todos.length;
    done += completed;
    const open = project.todos.length - completed;
    if (open > (mostOpen?.open ?? 0)) mostOpen = {project: project.name, open};
    for (const change of project.history) {
      changes.push({project: project.name, at: change.at, previous: statusSummary(change.previous), current: statusSummary(change.current)});
    }
  }
  changes.sort((a, b) => Date.parse(b.at) - Date.parse(a.at) || technologyOrder.compare(a.project, b.project));
  const ranked = Object.fromEntries(Object.entries(counts).map(([key, map]) => [key,
    Array.from(map, ([label, count]) => ({label, count})).sort((a, b) => b.count - a.count || technologyOrder.compare(a.label, b.label)),
  ]));
  return {
    projects: projects.length, folders: folders.length, starred, active, labelled,
    git_projects: git, with_remotes: remotes,
    since_last_scan: history.length > 1 ? history.at(-1).projects - history.at(-2).projects : null,
    todo: {total, done, open: total - done, most_open: mostOpen},
    ...ranked, last_report, scan_history: history, status_changes: changes,
  };
}

function statsCard(title, contents) {
  return `<section class="card"><h2 class="stats-title">${title}</h2>${contents}</section>`;
}

function statNumber(title, value, context) {
  return statsCard(title, `<div class="stat-number">${value}</div><div class="t3">${text(context)}</div>`);
}

function statsBars(entries, message = "No data yet.") {
  if (!entries.length) return `<div class="t3">${message}</div>`;
  return entries.slice(0, 6).map(({label, count}, index) => `<div class="row bar-row">
    <span class="t2 bar-label" title="${text(label)}">${text(label)}</span>
    <span class="bar-track" aria-hidden="true"><i class="${index === 0 ? "highlight" : ""}" style="width:${100 * count / entries[0].count}%"></i></span>
    <span class="bar-count">${count}</span>
  </div>`).join("");
}

function statsDonut(entries, total) {
  const portions = entries.length > 5 ? [...entries.slice(0, 4), {label: "Other", count: entries.slice(4).reduce((n, item) => n + item.count, 0)}] : entries;
  let start = 0;
  const circumference = 2 * Math.PI * 40;
  const arcs = portions.map(({count}, index) => {
    const share = count / total * circumference;
    const length = portions.length === 1 ? circumference : Math.max(0.15, share - 2);
    const arc = `<circle cx="60" cy="60" r="40" fill="none" stroke="${palette[index]}" stroke-width="12" stroke-dasharray="${length} ${circumference - length}" stroke-dashoffset="${-start}" transform="rotate(-90 60 60)"></circle>`;
    start += share;
    return arc;
  }).join("");
  return `<div class="row donut"><svg width="110" height="110" viewBox="0 0 120 120" aria-hidden="true">
    <circle cx="60" cy="60" r="40" fill="none" stroke="var(--hov)" stroke-width="12"></circle>${arcs}
    <text x="60" y="66" text-anchor="middle" font-size="20" font-weight="600" fill="var(--t1)">${total}</text>
  </svg><div class="legend">${portions.map(({label, count}, index) => `<div class="row legend-row">
    <i class="sq" style="background:${palette[index]}" aria-hidden="true"></i><span class="legend-label t2" title="${text(label)}">${text(label)}</span>${count}
  </div>`).join("") || '<span class="t3">No projects yet.</span>'}</div></div>`;
}

function scanActivity(history, report) {
  const scans = history.slice(-8);
  if (!scans.length && !report) return '<div class="t3">No completed scans yet.</div>';
  const highest = Math.max(1, ...scans.map(scan => scan.projects));
  const chart = scans.length ? `<svg width="100%" height="110" viewBox="0 0 350 104" role="img" aria-label="Projects per scan">
    <line x1="0" y1="101" x2="350" y2="101" stroke="var(--bd)"></line>${scans.map((scan, index) => {
      const x = 309 - (scans.length - index - 1) * 43;
      const height = Math.max(2, scan.projects / highest * 83);
      return `<g><title>${text(scan.at)} · ${scan.projects} projects</title><rect x="${x}" y="${100 - height}" width="28" height="${height}" rx="3" fill="${index === scans.length - 1 ? "var(--ac)" : "rgba(255,255,255,.28)"}"></rect>
        <text x="${x + 14}" y="${94 - height}" text-anchor="middle" font-size="10.5" fill="var(--t2)">${scan.projects}</text></g>`;
    }).join("")}</svg>` : "";
  const last = scans.at(-1);
  const facts = last ? `Last scan ${timeAgo(last.at)} · ${secondsText(last.elapsed_ms)} · ${last.folders} ${last.folders === 1 ? "folder" : "folders"}` : `Last scan · ${secondsText(report.elapsed_ms)}`;
  return `${chart}<div class="t3 scan-facts" title="${text(last?.at)}">${text(facts)}</div>${report ? `<div class="t3 scan-facts">${report.visited} directories visited · ${report.skipped} entries skipped</div>` : ""}`;
}

function sinceLastScan(delta, folders) {
  if (delta == null) return `${folders} scan ${folders === 1 ? "folder" : "folders"}`;
  return delta === 0 ? "No change since last scan" : `${delta > 0 ? "+" : "−"}${Math.abs(delta)} since last scan`;
}

function renderStatistics(stats) {
  const {projects, folders, starred, active, labelled, git_projects, with_remotes, since_last_scan, todo, project_types, languages, frameworks, statuses, git_hosts, owners, tags, versions, scan_history, last_report, status_changes} = stats;
  const pct = todo.total ? Math.round(todo.done * 100 / todo.total) : 0;
  const share = count => `${projects ? Math.round(count * 100 / projects) : 0}% of all`;
  return `<div class="stats-grid overview-cards">
    ${statNumber("Total projects", projects, sinceLastScan(since_last_scan, folders))}
    ${statNumber("Active", active, share(active))}
    ${statNumber("Starred", starred, share(starred))}
    ${statNumber("Git repositories", git_projects, `${git_projects - with_remotes} without remote`)}
    ${statNumber("Version-labelled", labelled, share(labelled))}
    ${statNumber("Open to-dos", todo.open, `${todo.done} of ${todo.total} done`)}
  </div>
  <div class="stats-grid breakdown-cards">
    ${statsCard("Projects by type", statsBars(project_types, "No projects yet."))}
    ${statsCard("Top languages", statsBars(languages))}
    ${statsCard("Top frameworks", statsBars(frameworks))}
  </div>
  <div class="stats-grid">
    ${statsCard("Project status", statsDonut(statuses, projects))}
    ${statsCard("Git hosts", statsDonut(git_hosts, projects))}
    ${statsCard("Owners / organizations", statsBars(owners))}
  </div>
  <div class="stats-grid scan-cards">
    ${statsCard("Scan activity", scanActivity(scan_history, last_report))}
    ${statsCard("Top tags", statsBars(tags, "No tags yet."))}
    ${statsCard("Version labels", versions.slice(0, 6).map(({label, count}, i) => `<div class="row version-entry">
      <span class="ch ellipsis" title="${text(label)}">${text(label)}</span><span class="t3 spacer">${i === 0 && count > 1 ? "most used" : /-[a-z]/i.test(label) ? "pre-release" : ""}</span>${count} ${count === 1 ? "project" : "projects"}
    </div>`).join("") || '<div class="t3">No version labels yet.</div>')}
  </div>
  <div class="stats-grid tracking-cards">
    ${statsCard("To-do completion", `<div class="stat-number">${pct}%<span class="t3"> tasks done</span></div>
      <div class="bar-track completion-bar" aria-hidden="true"><i class="highlight" style="width:${pct}%"></i></div>
      <div class="t3 completion-facts">${todo.done} done · ${todo.open} open · ${todo.total} total${todo.most_open ? `<br>Most open: ${text(todo.most_open.project)} (${todo.most_open.open})` : ""}</div>`)}
    ${statsCard("Recent status changes", status_changes.slice(0, 4).map(change => `<div class="row change-entry">
      <b class="ellipsis" title="${text(change.project)}">${text(change.project)}</b><span class="t2 spacer">${text(change.previous)} → ${text(change.current)}</span>
      <time class="t3 mono" datetime="${text(change.at)}" title="${text(change.at)} (UTC)">${text(formatUtc(change.at))}</time>
    </div>`).join("") || '<div class="t3">No status changes yet.</div>')}
  </div>`;
}
