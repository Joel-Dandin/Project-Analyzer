//! Additional project purposes derived from explicit, local manifest declarations.

use std::{fs, path::Path};

use serde_json::Value as Json;
use toml::Value as Toml;

use super::{files::is_link, model::Role};

pub(super) fn node(manifest: &Json) -> Option<Role> {
    if manifest.get("bin").is_some_and(|bin| {
        nonempty_json_string(bin)
            || bin
                .as_object()
                .is_some_and(|commands| commands.values().any(nonempty_json_string))
    }) {
        Some(Role::Cli)
    } else if manifest.get("private").and_then(Json::as_bool) != Some(true)
        && manifest.get("name").is_some_and(nonempty_json_string)
        && manifest
            .get("exports")
            .is_some_and(|exports| match exports {
                Json::Object(map) => !map.is_empty(),
                Json::Array(list) => !list.is_empty(),
                _ => nonempty_json_string(exports),
            })
    {
        Some(Role::Library)
    } else {
        None
    }
}

fn nonempty_json_string(value: &Json) -> bool {
    value.as_str().is_some_and(|value| !value.trim().is_empty())
}

pub(super) fn cargo(manifest: &Toml, path: &Path) -> Option<Role> {
    let package = manifest.get("package")?.as_table()?;
    let explicit_bin = manifest
        .get("bin")
        .and_then(Toml::as_array)
        .is_some_and(|bins| {
            bins.iter()
                .any(|bin| bin.get("name").or_else(|| bin.get("path")).is_some())
        });
    if explicit_bin {
        return Some(Role::Cli);
    }
    let root = path.parent()?;
    let has_main = package.get("autobins").and_then(Toml::as_bool) != Some(false)
        && regular_file(&root.join("src/main.rs"));
    if has_main
        && manifest
            .get("dependencies")
            .and_then(Toml::as_table)
            .is_some_and(|deps| {
                deps.iter().any(|(key, value)| {
                    matches!(
                        value.get("package").and_then(Toml::as_str).unwrap_or(key),
                        "clap" | "argh" | "bpaf" | "structopt" | "gumdrop" | "pico-args"
                    )
                })
            })
    {
        return Some(Role::Cli);
    }
    let has_lib = manifest.get("lib").is_some_and(Toml::is_table)
        || (package.get("autolib").and_then(Toml::as_bool) != Some(false)
            && regular_file(&root.join("src/lib.rs")));
    (has_lib && !has_main).then_some(Role::Library)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && !is_link(&meta))
}

pub(super) fn python(manifest: &Toml) -> Option<Role> {
    let project = manifest.get("project");
    let tool = manifest.get("tool");
    let poetry = tool.and_then(|tool| tool.get("poetry"));
    if project.into_iter().chain(poetry).any(|section| {
        section
            .get("scripts")
            .and_then(Toml::as_table)
            .is_some_and(|scripts| !scripts.is_empty())
    }) {
        Some(Role::Cli)
    } else if poetry
        .and_then(|section| section.get("packages"))
        .and_then(Toml::as_array)
        .is_some_and(|packages| !packages.is_empty())
        || tool
            .and_then(|tool| tool.get("setuptools"))
            .and_then(|section| section.get("packages"))
            .is_some_and(|packages| {
                packages.as_array().is_some_and(|list| !list.is_empty())
                    || packages
                        .as_table()
                        .is_some_and(|table| table.contains_key("find"))
            })
    {
        Some(Role::Library)
    } else {
        None
    }
}

pub(super) fn terraform(name: &str, source: &str) -> bool {
    const BLOCKS: &[&str] = &[
        "terraform",
        "provider",
        "resource",
        "module",
        "data",
        "variable",
        "output",
        "locals",
    ];
    if name.ends_with(".tf.json") {
        return serde_json::from_str::<Json>(source)
            .ok()
            .is_some_and(|data| {
                BLOCKS
                    .iter()
                    .any(|block| data.get(block).is_some_and(Json::is_object))
            });
    }
    // Recognize block headers without parsing or evaluating HCL expressions.
    source.lines().map(str::trim).any(|line| {
        line.contains('{')
            && line
                .split(|ch: char| ch.is_ascii_whitespace() || ch == '{')
                .next()
                .is_some_and(|token| BLOCKS.contains(&token))
    })
}

pub(super) fn helm(source: &str) -> bool {
    let Ok(chart) = serde_yaml::from_str::<serde_yaml::Value>(source) else {
        return false;
    };
    matches!(
        chart.get("apiVersion").and_then(serde_yaml::Value::as_str),
        Some("v1" | "v2")
    ) && ["name", "version"].into_iter().all(|field| {
        chart
            .get(field)
            .and_then(serde_yaml::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}
