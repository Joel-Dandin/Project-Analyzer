"use strict";

// Keys match the dashboard data sent by Rust. Filter membership can overlap, but
// a project appears in exactly one group in the inventory and statistics.
const projectTypes = Object.freeze({
  frontend: "Frontend", backend: "Backend", full_stack: "Full-stack",
  android: "Android only", ios: "iOS only", dual: "Android + iOS",
  groovy: "Gradle (Groovy DSL)", kotlin: "Gradle (Kotlin DSL)",
  gradle_mixed: "Gradle (both DSLs)", desktop: "Desktop", mobile: "Other mobile", tooling: "General / tooling",
});
const filters = Object.freeze({
  ...projectTypes, android_any: "Android (any)", ios_any: "iOS (any)", gradle: "Non-mobile Gradle (any DSL)",
});
const statuses = Object.freeze({
  planned: ["Planning", "#8A8F98"], in_progress: ["Active", "#4CB782"],
  developing: ["Developing", "#4CB782"], in_review: ["In review", "#F2C94C"],
  on_hold: ["On hold", "#EB5757"], completed: ["Completed", "#4CB782"],
  archived: ["Archived", "#6B6F76"], cancelled: ["Cancelled", "#EB5757"],
});
const statusOrder = ["in_progress", "developing", "planned", "in_review", "on_hold", "completed", "archived", "cancelled"];
const utc = new Intl.DateTimeFormat("en-GB", {
  day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23", timeZone: "UTC",
});
const technologyOrder = new Intl.Collator("en", {numeric: true, sensitivity: "base"});

function text(value) {
  return String(value ?? "").replace(/[&<>"']/g, c => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"})[c]);
}

function matchesType(project, type) {
  switch (type) {
    case "": return true;
    case "android_any": return project.kind === "android" || project.kind === "dual";
    case "ios_any": return project.kind === "ios" || project.kind === "dual";
    case "gradle": return project.groovy || project.kotlin;
    case "groovy": return project.groovy;
    case "kotlin": return project.kotlin;
    default: return project.kind === type;
  }
}

function primaryRemote(project) {
  const remotes = project.repository?.remotes ?? [];
  return remotes.find(remote => remote.name === "origin") ?? remotes[0];
}

function statusLabel(value) {
  return Object.hasOwn(statuses, value) ? statuses[value][0] : "Not set";
}

function statusSummary(values) {
  return values.length ? values.map(statusLabel).join(", ") : "Not set";
}

function primaryStatus(values) {
  return statusOrder.find(value => values.includes(value));
}

function statusDot(values, inline = false) {
  const status = primaryStatus(values);
  return `<i class="dot${inline ? " inline-dot" : ""}" style="background:${statuses[status]?.[1] ?? "#6B6F76"}" title="${text(statusSummary(values))}" aria-hidden="true"></i>`;
}

function matchesProject(project, {kind, starred, version, query}) {
  if (!matchesType(project, kind) || (starred && !project.starred)) return false;
  if (version && !project.versions.some(label => label.toLowerCase().includes(version))) return false;
  if (!query) return true;
  const remotes = (project.repository?.remotes ?? []).flatMap(remote => [
    remote.name, remote.host, remote.account, remote.repository,
    ...((remote.push_targets ?? []).flatMap(target => [target.host, target.account, target.repository])),
  ]);
  const haystack = [project.name, project.path, projectTypes[project.kind], ...project.stack, ...remotes,
    project.summary, project.notes, ...project.tags, ...project.versions,
    ...project.statuses.flatMap(value => [statusLabel(value), value.replaceAll("_", " ")]),
  ].join("\n").toLowerCase();
  return query.split(/\s+/).every(term => haystack.includes(term));
}

function orderProjects(a, b) {
  return technologyOrder.compare(a.name, b.name) || technologyOrder.compare(a.path, b.path) || a.id.localeCompare(b.id);
}

function shortPath(path, home, windows) {
  if (!home) return path;
  const bareHome = home.replace(/[\\/]$/, "");
  const normalize = value => windows ? value.replaceAll("\\", "/").toLowerCase() : value;
  const root = normalize(bareHome);
  const candidate = normalize(path);
  if (candidate === root || candidate.startsWith(`${root}/`)) return `~${path.slice(bareHome.length)}`;
  return path;
}

function formatUtc(at) {
  const date = new Date(at);
  return Number.isFinite(date.getTime()) ? utc.format(date) : "Unknown time";
}

function timeAgo(at, now = Date.now()) {
  const seconds = Math.max(0, (now - Date.parse(at)) / 1000);
  if (!Number.isFinite(seconds)) return "previously";
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

function secondsText(elapsedMs) {
  return elapsedMs < 100 ? "<0.1s" : `${Number((elapsedMs / 1000).toFixed(1))}s`;
}
