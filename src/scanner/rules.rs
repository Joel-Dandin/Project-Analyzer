//! Filename gates followed by bounded manifest or source inspection.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering::Relaxed},
};

use serde_json::Value as Json;
use toml::Value as Toml;

use super::{
    files::read_small_text,
    model::{ANDROID, Component, GRADLE, GRADLE_KOTLIN, IOS, Role},
};

const MANIFEST_LIMIT: u64 = 256 * 1024;
const XCODE_LIMIT: u64 = 1024 * 1024;

#[derive(Default)]
pub(super) struct Evidence {
    pub roles: BTreeSet<Role>,
    pub stack: BTreeSet<String>,
    pub signals: BTreeSet<String>,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub declared: bool,
}

impl Evidence {
    fn stack(&mut self, label: &str) {
        self.stack.insert(label.to_owned());
    }

    fn role(&mut self, role: Role, label: &str) {
        self.roles.insert(role);
        self.stack(label);
    }

    fn signal(&mut self, name: &str) {
        self.signals.insert(name.to_owned());
    }

    fn declare(&mut self, name: &str, language: &str) {
        self.declared = true;
        self.signal(name);
        self.stack(language);
    }

    pub fn into_component(self, path: PathBuf) -> Component {
        Component {
            path,
            roles: self.roles,
            stack: self.stack,
            signals: self.signals,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Rule {
    Node,
    Pnpm,
    Lerna,
    Angular,
    Next,
    Svelte,
    Nuxt,
    Tauri,
    Html,
    JavascriptSource,
    Cargo,
    PythonToml,
    PythonRequirements,
    Pipfile,
    PythonSetup,
    PythonSource,
    Maven,
    Gradle,
    GradleSettings,
    AndroidManifest,
    Xcode,
    Go,
    GoWorkspace,
    GoSource,
    Dotnet,
    Dart,
    Ruby,
    Php,
    Swift,
    Elixir,
}

impl Rule {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "package.json" => Self::Node,
            "pnpm-workspace.yaml" => Self::Pnpm,
            "lerna.json" => Self::Lerna,
            "angular.json" => Self::Angular,
            "next.config.js" | "next.config.mjs" | "next.config.ts" | "next.config.cjs" => {
                Self::Next
            }
            "svelte.config.js" | "svelte.config.ts" | "svelte.config.mjs" => Self::Svelte,
            "nuxt.config.js" | "nuxt.config.ts" | "nuxt.config.mjs" => Self::Nuxt,
            "tauri.conf.json" | "tauri.conf.json5" | "Tauri.toml" => Self::Tauri,
            "index.html" => Self::Html,
            "main.js" | "main.ts" | "server.js" | "server.ts" | "index.jsx" | "index.tsx"
            | "main.jsx" | "main.tsx" => Self::JavascriptSource,
            "Cargo.toml" => Self::Cargo,
            "pyproject.toml" => Self::PythonToml,
            "requirements.txt" | "requirements-dev.txt" | "requirements.in" => {
                Self::PythonRequirements
            }
            "Pipfile" => Self::Pipfile,
            "setup.py" | "setup.cfg" => Self::PythonSetup,
            "app.py" | "manage.py" | "main.py" | "server.py" | "wsgi.py" | "asgi.py" | "run.py"
            | "__init__.py" => Self::PythonSource,
            "pom.xml" => Self::Maven,
            "build.gradle" | "build.gradle.kts" => Self::Gradle,
            "settings.gradle" | "settings.gradle.kts" => Self::GradleSettings,
            "AndroidManifest.xml" => Self::AndroidManifest,
            "project.pbxproj" => Self::Xcode,
            "go.mod" => Self::Go,
            "go.work" => Self::GoWorkspace,
            "main.go" => Self::GoSource,
            "pubspec.yaml" => Self::Dart,
            "Gemfile" => Self::Ruby,
            "composer.json" => Self::Php,
            "Package.swift" => Self::Swift,
            "mix.exs" => Self::Elixir,
            _ if name.ends_with(".csproj") || name.ends_with(".fsproj") => Self::Dotnet,
            _ => return None,
        })
    }

    fn needs_content(self) -> bool {
        !matches!(
            self,
            Self::Angular | Self::Next | Self::Svelte | Self::Nuxt | Self::Tauri | Self::Html
        )
    }
}

pub(super) struct Candidate {
    pub path: PathBuf,
    pub rule: Rule,
}

impl Candidate {
    pub fn directory(&self) -> Option<&Path> {
        let parent = self.path.parent()?;
        match self.rule {
            Rule::AndroidManifest => {
                // <module>/src/<source set>/AndroidManifest.xml belongs to the module.
                // A top-level manifest is also valid in older projects.
                if let Some(src) = parent
                    .parent()
                    .filter(|dir| dir.file_name().is_some_and(|name| name == "src"))
                {
                    src.parent()
                } else {
                    Some(parent)
                }
            }
            Rule::Xcode => parent
                .file_name()?
                .to_str()?
                .ends_with(".xcodeproj")
                .then(|| parent.parent())
                .flatten(),
            _ => Some(parent),
        }
    }
}

pub(super) fn inspect(files: &[Candidate], cancelled: &AtomicBool) -> (Evidence, usize) {
    let mut evidence = Evidence::default();
    let mut failures = 0;
    for Candidate { path, rule } in files {
        if cancelled.load(Relaxed) {
            break;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !rule.needs_content() {
            match rule {
                Rule::Angular => evidence.role(Role::Frontend, "Angular"),
                Rule::Next => evidence.role(Role::Frontend, "Next.js"),
                Rule::Svelte => evidence.role(Role::Frontend, "Svelte"),
                Rule::Nuxt => evidence.role(Role::Frontend, "Nuxt"),
                Rule::Tauri => evidence.role(Role::Desktop, "Tauri"),
                Rule::Html if standalone_html(path) => evidence.role(Role::Frontend, "HTML"),
                _ => continue,
            }
            evidence.declared = true;
            evidence.signal(name);
            continue;
        }
        let limit = if matches!(rule, Rule::Xcode) {
            XCODE_LIMIT
        } else {
            MANIFEST_LIMIT
        };
        match read_small_text(path, limit) {
            Ok(text) => apply(*rule, name, &text, &mut evidence),
            Err(_) => failures += 1,
        }
    }
    (evidence, failures)
}

fn standalone_html(path: &Path) -> bool {
    !path
        .parent()
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        .is_some_and(|s| {
            matches!(
                s,
                "public" | "static" | "templates" | "assets" | "docs" | "wwwroot" | "resources"
            )
        })
}

fn apply(rule: Rule, name: &str, text: &str, evidence: &mut Evidence) {
    match rule {
        Rule::Node => {
            if let Ok(data) = serde_json::from_str::<Json>(text)
                && let Some(object) = data.as_object()
            {
                evidence.declare(name, "Node.js");
                for key in [
                    "dependencies",
                    "devDependencies",
                    "peerDependencies",
                    "optionalDependencies",
                ] {
                    if let Some(deps) = object.get(key).and_then(Json::as_object) {
                        for package in deps.keys() {
                            node_dependency(package, evidence);
                        }
                    }
                }
                if let Some(workspaces) = data.get("workspaces") {
                    json_patterns(
                        workspaces.get("packages").unwrap_or(workspaces),
                        &mut evidence.includes,
                    );
                }
            }
        }
        Rule::Pnpm => {
            if let Ok(data) = serde_yaml::from_str::<serde_yaml::Value>(text)
                && let Some(packages) = data
                    .get("packages")
                    .and_then(serde_yaml::Value::as_sequence)
            {
                evidence.declare(name, "pnpm");
                evidence.includes.extend(
                    packages
                        .iter()
                        .filter_map(serde_yaml::Value::as_str)
                        .map(str::to_owned),
                );
            }
        }
        Rule::Lerna => {
            if let Ok(data) = serde_json::from_str::<Json>(text)
                && let Some(packages) = data.get("packages")
            {
                evidence.declare(name, "Lerna");
                json_patterns(packages, &mut evidence.includes);
            }
        }
        Rule::JavascriptSource => {
            let mut found = false;
            for line in code_lines(text) {
                for (package, label) in [("react-dom/client", "React"), ("vue", "Vue")] {
                    if module_reference(line, package) {
                        evidence.role(Role::Frontend, label);
                        found = true;
                    }
                }
                for (package, label) in [
                    ("express", "Express"),
                    ("fastify", "Fastify"),
                    ("koa", "Koa"),
                ] {
                    if module_reference(line, package) {
                        evidence.role(Role::Backend, label);
                        found = true;
                    }
                }
            }
            if found {
                evidence.signal(name);
            }
        }
        Rule::Cargo => {
            if let Ok(data) = text.parse::<Toml>()
                && data.is_table()
                && (data.get("package").is_some() || data.get("workspace").is_some())
            {
                evidence.declare(name, "Rust");
                if let Some(workspace) = data.get("workspace") {
                    toml_patterns(workspace.get("members"), &mut evidence.includes);
                    toml_patterns(workspace.get("exclude"), &mut evidence.excludes);
                    rust_dependencies(workspace, evidence);
                }
                rust_dependencies(&data, evidence);
                if let Some(targets) = data.get("target").and_then(Toml::as_table) {
                    for target in targets.values() {
                        rust_dependencies(target, evidence);
                    }
                }
            }
        }
        Rule::PythonToml | Rule::Pipfile => {
            if let Ok(data) = text.parse::<Toml>()
                && data.is_table()
                && (data.get("project").is_some()
                    || data.get("build-system").is_some()
                    || data.get("tool").and_then(|d| d.get("poetry")).is_some()
                    || matches!(rule, Rule::Pipfile))
            {
                evidence.declare(name, "Python");
                if let Some(project) = data.get("project") {
                    python_array(project.get("dependencies"), evidence);
                    if let Some(groups) = project
                        .get("optional-dependencies")
                        .and_then(Toml::as_table)
                    {
                        for group in groups.values() {
                            python_array(Some(group), evidence);
                        }
                    }
                }
                if let Some(poetry) = data.get("tool").and_then(|d| d.get("poetry")) {
                    python_keys(poetry.get("dependencies"), evidence);
                }
                python_keys(data.get("packages"), evidence);
                toml_patterns(
                    data.get("tool")
                        .and_then(|d| d.get("uv"))
                        .and_then(|d| d.get("workspace"))
                        .and_then(|d| d.get("members")),
                    &mut evidence.includes,
                );
                toml_patterns(
                    data.get("tool")
                        .and_then(|d| d.get("uv"))
                        .and_then(|d| d.get("workspace"))
                        .and_then(|d| d.get("exclude")),
                    &mut evidence.excludes,
                );
            }
        }
        Rule::PythonRequirements | Rule::PythonSetup => {
            evidence.declare(name, "Python");
            for line in code_lines(text) {
                python_dependency(line.trim_start_matches(['\'', '"']), evidence);
            }
        }
        Rule::PythonSource => {
            let mut found = false;
            for line in code_lines(text) {
                for package in [
                    "flask",
                    "django",
                    "fastapi",
                    "starlette",
                    "streamlit",
                    "gradio",
                    "aiohttp",
                    "tkinter",
                    "PySide6",
                    "PyQt6",
                ] {
                    if python_import(line, package) {
                        python_dependency(package, evidence);
                        found = true;
                    }
                }
                if line.starts_with("if __name__ ==") && line.contains("__main__") {
                    evidence.stack("Python");
                    found = true;
                }
            }
            if found {
                evidence.signal(name);
            }
        }
        Rule::Maven => {
            if let Ok(doc) = roxmltree::Document::parse(text)
                && doc.root_element().has_tag_name("project")
            {
                evidence.declare(name, "Java");
                for node in doc
                    .root_element()
                    .descendants()
                    .filter(roxmltree::Node::is_element)
                {
                    let value = node.text().unwrap_or_default().trim();
                    match node.tag_name().name() {
                        "module" if node.parent().is_some_and(|p| p.has_tag_name("modules")) => {
                            evidence.includes.push(value.to_owned());
                        }
                        "groupId" if value == "org.springframework.boot" => {
                            evidence.role(Role::Backend, "Spring Boot")
                        }
                        "groupId" if value == "io.quarkus" => {
                            evidence.role(Role::Backend, "Quarkus")
                        }
                        "groupId" if value.starts_with("io.micronaut") => {
                            evidence.role(Role::Backend, "Micronaut")
                        }
                        _ => {}
                    }
                }
            }
        }
        Rule::Gradle | Rule::GradleSettings => {
            evidence.declare(
                name,
                if name.ends_with(".kts") {
                    GRADLE_KOTLIN
                } else {
                    GRADLE
                },
            );
            for line in code_lines(text) {
                if line.contains("org.springframework.boot") {
                    evidence.role(Role::Backend, "Spring Boot");
                }
                if line.contains("io.quarkus") {
                    evidence.role(Role::Backend, "Quarkus");
                }
                if matches!(rule, Rule::Gradle) && android_plugin(line) {
                    evidence.role(Role::Mobile, ANDROID);
                }
                if matches!(rule, Rule::GradleSettings)
                    && let Some(rest) = line.strip_prefix("include")
                    && rest.starts_with([' ', '(', '\t'])
                {
                    for member in rest.split(['\'', '"']).skip(1).step_by(2) {
                        evidence
                            .includes
                            .push(member.trim_start_matches(':').replace(':', "/"));
                    }
                }
            }
        }
        Rule::AndroidManifest => {
            if roxmltree::Document::parse(text)
                .is_ok_and(|document| document.root_element().has_tag_name("manifest"))
            {
                evidence.declare(name, ANDROID);
                evidence.role(Role::Mobile, ANDROID);
            }
        }
        Rule::Xcode => {
            evidence.declare(name, "Xcode");
            if code_lines(text)
                .flat_map(|line| {
                    line.split("//")
                        .next()
                        .unwrap_or(line)
                        .split(['{', '}', ';'])
                })
                .any(ios_build_setting)
            {
                evidence.role(Role::Mobile, IOS);
            }
        }
        Rule::Go => {
            if code_lines(text).any(|line| line.starts_with("module ")) {
                evidence.declare(name, "Go");
                for (package, label) in [
                    ("github.com/gin-gonic/gin", "Gin"),
                    ("github.com/labstack/echo", "Echo"),
                    ("github.com/gofiber/fiber", "Fiber"),
                ] {
                    if code_lines(text).any(|line| {
                        line.split_whitespace().any(|part| {
                            part == package
                                || part
                                    .strip_prefix(package)
                                    .is_some_and(|s| s.starts_with("/v"))
                        })
                    }) {
                        evidence.role(Role::Backend, label);
                    }
                }
            }
        }
        Rule::GoWorkspace => {
            evidence.declare(name, "Go");
            let mut in_block = false;
            for line in code_lines(text) {
                let line = line.split("//").next().unwrap_or_default().trim();
                if let Some(path) = line.strip_prefix("use ") {
                    if path.trim() == "(" {
                        in_block = true;
                    } else {
                        evidence
                            .includes
                            .push(path.trim().trim_matches('"').to_owned());
                    }
                } else if line == ")" {
                    in_block = false;
                } else if in_block && !line.is_empty() {
                    evidence.includes.push(line.trim_matches('"').to_owned());
                }
            }
        }
        Rule::GoSource => {
            if code_lines(text).any(|line| {
                line.contains("http.ListenAndServe(") || line.contains("http.ListenAndServeTLS(")
            }) {
                evidence.role(Role::Backend, "Go HTTP");
                evidence.signal(name);
            }
        }
        Rule::Dotnet => {
            if let Ok(doc) = roxmltree::Document::parse(text)
                && doc.root_element().has_tag_name("Project")
            {
                evidence.declare(
                    name,
                    if name.ends_with(".fsproj") {
                        "F#"
                    } else {
                        "C#"
                    },
                );
                evidence.stack(".NET");
                if doc
                    .root_element()
                    .attribute("Sdk")
                    .is_some_and(|sdk| sdk.contains("Microsoft.NET.Sdk.Web"))
                {
                    evidence.role(Role::Backend, "ASP.NET Core");
                }
                for node in doc
                    .root_element()
                    .descendants()
                    .filter(roxmltree::Node::is_element)
                {
                    if node
                        .text()
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("true"))
                    {
                        match node.tag_name().name() {
                            "UseWPF" => evidence.role(Role::Desktop, "WPF"),
                            "UseWindowsForms" => evidence.role(Role::Desktop, "WinForms"),
                            "UseMaui" => evidence.role(Role::Mobile, ".NET MAUI"),
                            _ => {}
                        }
                    }
                }
            }
        }
        Rule::Dart => {
            if let Ok(data) = serde_yaml::from_str::<serde_yaml::Value>(text)
                && data.get("name").is_some()
            {
                evidence.declare(name, "Dart");
                if data
                    .get("dependencies")
                    .and_then(|d| d.get("flutter"))
                    .is_some()
                {
                    evidence.role(Role::Mobile, "Flutter");
                }
            }
        }
        Rule::Ruby => {
            evidence.declare(name, "Ruby");
            for line in code_lines(text) {
                if let Some(name) = line.strip_prefix("gem ").map(str::trim_start) {
                    match name.split(['\'', '"']).nth(1) {
                        Some("rails") => evidence.role(Role::Backend, "Ruby on Rails"),
                        Some("sinatra") => evidence.role(Role::Backend, "Sinatra"),
                        _ => {}
                    }
                }
            }
        }
        Rule::Php => {
            if let Ok(data) = serde_json::from_str::<Json>(text)
                && data.is_object()
            {
                evidence.declare(name, "PHP");
                if let Some(deps) = data.get("require") {
                    if deps.get("laravel/framework").is_some() {
                        evidence.role(Role::Backend, "Laravel");
                    }
                    if deps.get("symfony/framework-bundle").is_some() {
                        evidence.role(Role::Backend, "Symfony");
                    }
                }
            }
        }
        Rule::Swift => {
            if text.contains("PackageDescription") {
                evidence.declare(name, "Swift");
            }
        }
        Rule::Elixir => {
            evidence.declare(name, "Elixir");
            if code_lines(text).any(|line| line.contains("{:phoenix,")) {
                evidence.role(Role::Backend, "Phoenix");
            }
        }
        Rule::Angular | Rule::Next | Rule::Svelte | Rule::Nuxt | Rule::Tauri | Rule::Html => {}
    }
}

fn android_plugin(line: &str) -> bool {
    line.split("//")
        .next()
        .unwrap_or(line)
        .split(['{', '}', ';'])
        .any(applies_android_plugin)
}

fn applies_android_plugin(statement: &str) -> bool {
    let Some(at) = statement.find(['\'', '"']) else {
        return false;
    };
    let (prefix, quoted) = statement.split_at(at);
    let mut declaration = prefix
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ':' | '='))
        .filter(|token| !token.is_empty());
    if !matches!(
        (declaration.next(), declaration.next(), declaration.next()),
        (Some("id"), None, None) | (Some("apply"), Some("plugin"), None)
    ) {
        return false;
    }

    let Some((id, suffix)) = quoted[1..].split_once(char::from(quoted.as_bytes()[0])) else {
        return false;
    };
    if !matches!(
        id,
        "com.android.application" | "com.android.library" | "com.android.dynamic-feature"
    ) {
        return false;
    }

    // A root build can declare an Android plugin version without applying it.
    let tokens = suffix
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')'))
        .filter(|token| !token.is_empty());
    !tokens
        .clone()
        .zip(tokens.skip(1))
        .any(|pair| pair == ("apply", "false"))
}

fn ios_build_setting(line: &str) -> bool {
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };
    match key.trim().trim_matches('"') {
        "IPHONEOS_DEPLOYMENT_TARGET" => true,
        "SDKROOT" | "SUPPORTED_PLATFORMS" => value
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|platform| matches!(platform, "iphoneos" | "iphonesimulator")),
        _ => false,
    }
}

fn node_dependency(package: &str, evidence: &mut Evidence) {
    match package {
        "react" | "react-dom" => evidence.role(Role::Frontend, "React"),
        "react-native" => evidence.role(Role::Mobile, "React Native"),
        "expo" => evidence.role(Role::Mobile, "Expo"),
        "next" => evidence.role(Role::Frontend, "Next.js"),
        "vue" => evidence.role(Role::Frontend, "Vue"),
        "nuxt" => evidence.role(Role::Frontend, "Nuxt"),
        "svelte" | "@sveltejs/kit" => evidence.role(Role::Frontend, "Svelte"),
        "@angular/core" => evidence.role(Role::Frontend, "Angular"),
        "@solidjs/start" | "solid-js" => evidence.role(Role::Frontend, "SolidJS"),
        "astro" => evidence.role(Role::Frontend, "Astro"),
        "express" => evidence.role(Role::Backend, "Express"),
        "fastify" => evidence.role(Role::Backend, "Fastify"),
        "koa" => evidence.role(Role::Backend, "Koa"),
        "@nestjs/core" => evidence.role(Role::Backend, "NestJS"),
        "hono" => evidence.role(Role::Backend, "Hono"),
        "electron" => evidence.role(Role::Desktop, "Electron"),
        "@tauri-apps/api" | "@tauri-apps/cli" => evidence.role(Role::Desktop, "Tauri"),
        "typescript" => evidence.stack("TypeScript"),
        "vite" => evidence.stack("Vite"),
        _ => {}
    }
}

fn rust_dependencies(manifest: &Toml, evidence: &mut Evidence) {
    for section in ["dependencies", "build-dependencies"] {
        if let Some(deps) = manifest.get(section).and_then(Toml::as_table) {
            for (key, value) in deps {
                match value.get("package").and_then(Toml::as_str).unwrap_or(key) {
                    "tauri" | "tauri-build" => evidence.role(Role::Desktop, "Tauri"),
                    "gpui" => evidence.role(Role::Desktop, "GPUI"),
                    "eframe" | "egui" => evidence.role(Role::Desktop, "egui"),
                    "iced" => evidence.role(Role::Desktop, "Iced"),
                    "axum" => evidence.role(Role::Backend, "Axum"),
                    "actix-web" => evidence.role(Role::Backend, "Actix Web"),
                    "rocket" => evidence.role(Role::Backend, "Rocket"),
                    "yew" => evidence.role(Role::Frontend, "Yew"),
                    _ => {}
                }
            }
        }
    }
}

fn python_dependency(spec: &str, evidence: &mut Evidence) {
    let package = spec
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .next()
        .unwrap_or_default();
    let (role, label) = match package.to_ascii_lowercase().as_str() {
        "flask" => (Role::Backend, "Flask"),
        "fastapi" => (Role::Backend, "FastAPI"),
        "django" => (Role::Backend, "Django"),
        "starlette" => (Role::Backend, "Starlette"),
        "aiohttp" => (Role::Backend, "aiohttp"),
        "streamlit" => (Role::Frontend, "Streamlit"),
        "gradio" => (Role::Frontend, "Gradio"),
        "tkinter" => (Role::Desktop, "Tkinter"),
        "pyside6" | "pyqt6" => (Role::Desktop, "Qt for Python"),
        _ => return,
    };
    evidence.stack("Python");
    evidence.role(role, label);
}

fn python_keys(deps: Option<&Toml>, evidence: &mut Evidence) {
    if let Some(table) = deps.and_then(Toml::as_table) {
        for key in table.keys() {
            python_dependency(key, evidence);
        }
    }
}

fn python_array(deps: Option<&Toml>, evidence: &mut Evidence) {
    if let Some(list) = deps.and_then(Toml::as_array) {
        for spec in list.iter().filter_map(Toml::as_str) {
            python_dependency(spec, evidence);
        }
    }
}

fn python_import(line: &str, package: &str) -> bool {
    for prefix in ["from ", "import "] {
        if let Some(rest) = line.strip_prefix(prefix).map(str::trim_start)
            && let Some(rest) = rest.strip_prefix(package)
            && (rest.is_empty() || rest.starts_with(['.', ' ', ',', '\t']))
        {
            return true;
        }
    }
    false
}

fn module_reference(line: &str, package: &str) -> bool {
    (line.starts_with("import ") || line.contains("require("))
        && (line.contains(&format!("'{package}'")) || line.contains(&format!("\"{package}\"")))
}

fn json_patterns(value: &Json, output: &mut Vec<String>) {
    if let Some(items) = value.as_array() {
        output.extend(items.iter().filter_map(Json::as_str).map(str::to_owned));
    }
}

fn toml_patterns(value: Option<&Toml>, output: &mut Vec<String>) {
    if let Some(items) = value.and_then(Toml::as_array) {
        output.extend(items.iter().filter_map(Toml::as_str).map(str::to_owned));
    }
}

fn code_lines(source: &str) -> impl Iterator<Item = &str> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', ';']) && !line.starts_with("//"))
}
