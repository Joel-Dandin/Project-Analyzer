use std::{fs, path::Path};

use anyhow::{Context as _, Result};
use project_analyser::scanner::{ProjectKind, ScanControl, scan};

#[test]
fn classifies_explicit_purposes_without_mistaking_helpers_for_primary_apps() -> Result<()> {
    use ProjectKind::*;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    type Fixture = (
        &'static str,
        ProjectKind,
        &'static [(&'static str, &'static str)],
    );
    let fixtures: &[Fixture] = &[
        (
            "node-sdk",
            Library,
            &[(
                "package.json",
                r#"{"name":"sample-sdk","exports":{".":"./index.js"}}"#,
            )],
        ),
        (
            "private-module",
            Tooling,
            &[(
                "package.json",
                r#"{"private":true,"name":"private-module","exports":"./index.js"}"#,
            )],
        ),
        (
            "rust-sdk",
            Library,
            &[
                ("Cargo.toml", "[package]\nname='sample'\nversion='0.1.0'\n"),
                ("src/lib.rs", "pub fn ping() {}"),
            ],
        ),
        (
            "node-cli",
            Cli,
            &[(
                "package.json",
                r#"{"name":"sample","bin":{"sample":"./cli.js"},"exports":"./index.js"}"#,
            )],
        ),
        (
            "rust-cli",
            Cli,
            &[
                (
                    "Cargo.toml",
                    "[package]\nname='sample'\nversion='0.1.0'\n[[bin]]\nname='sample'\npath='src/main.rs'\n",
                ),
                ("src/main.rs", "fn main() {}"),
            ],
        ),
        (
            "rust-clap",
            Cli,
            &[
                (
                    "Cargo.toml",
                    "[package]\nname='sample'\nversion='0.1.0'\n[dependencies]\nargs = { package = 'clap', version = '4' }\n",
                ),
                ("src/main.rs", "fn main() {}"),
            ],
        ),
        (
            "rust-desktop",
            Desktop,
            &[
                (
                    "Cargo.toml",
                    "[package]\nname='sample'\nversion='0.1.0'\n[dependencies]\nwry='0.55'\nclap='4'\n",
                ),
                ("src/main.rs", "fn main() {}"),
            ],
        ),
        (
            "python-cli",
            Cli,
            &[(
                "pyproject.toml",
                "[project]\nname='sample'\n[project.scripts]\nsample='sample:main'\n",
            )],
        ),
        (
            "python-sdk",
            Library,
            &[(
                "pyproject.toml",
                "[project]\nname='sample'\n[tool.setuptools.packages.find]\nwhere=['src']\n",
            )],
        ),
        (
            "model-training",
            Data,
            &[("requirements.txt", "numpy>=2\nscikit-learn>=1\n")],
        ),
        (
            "not-data",
            Tooling,
            &[("requirements.txt", "requests\nnumpy\n")],
        ),
        (
            "terraform",
            Infrastructure,
            &[("main.tf", "resource \"sample_resource\" \"example\" {\n}\n")],
        ),
        (
            "terraform-json",
            Infrastructure,
            &[("main.tf.json", r#"{"variable":{"name":{"type":"string"}}}"#)],
        ),
        (
            "helm",
            Infrastructure,
            &[(
                "Chart.yaml",
                "apiVersion: v2\nname: sample\nversion: 0.1.0\n",
            )],
        ),
        (
            "inference-service",
            Backend,
            &[
                ("requirements.txt", "fastapi\ntorch\n"),
                ("main.tf", "terraform {\n}\n"),
            ],
        ),
        (
            "web-with-helper",
            Frontend,
            &[(
                "package.json",
                r#"{"name":"app","bin":"./tool.js","dependencies":{"react":"19"}}"#,
            )],
        ),
        (
            "gradle-kotlin",
            GradleKotlin,
            &[
                ("build.gradle.kts", "plugins { java }\n"),
                ("main.tf", "terraform {\n}\n"),
            ],
        ),
        (
            "gradle-groovy",
            GradleGroovy,
            &[("build.gradle", "plugins { id 'java' }\n")],
        ),
    ];
    for &(name, _, files) in fixtures {
        for &(file, content) in files {
            put(&root.join(name).join(file), content)?;
        }
    }
    // Arbitrarily named infrastructure files, an empty chart, and a Dockerfile
    // alone should not invent projects.
    put(
        &root.join("not-project/foo.tf"),
        "# resource example\nnot a manifest\n",
    )?;
    put(&root.join("not-chart/Chart.yaml"), "name: not-a-chart\n")?;
    put(&root.join("build-helper/Dockerfile"), "FROM scratch\n")?;

    let report = scan(root, &ScanControl::default())?.context("scan cancelled")?;
    assert_eq!(report.projects.len(), fixtures.len(), "{report:#?}");
    for &(name, expected, _) in fixtures {
        let project = report
            .projects
            .iter()
            .find(|project| project.name() == name)
            .with_context(|| name.to_owned())?;
        assert_eq!(project.classification(), expected, "{name}");
        assert_eq!(project.kind(), expected.label(), "{name}");
    }
    Ok(())
}

fn put(path: &Path, content: &str) -> Result<()> {
    fs::create_dir_all(path.parent().context("fixture path missing parent")?)?;
    fs::write(path, content)?;
    Ok(())
}
