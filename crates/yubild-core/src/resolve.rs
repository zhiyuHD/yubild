//! 配置校验与依赖排序。

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::model::{Config, PostBuildStep, TargetKind};

/// 校验配置的整体一致性。
pub fn validate(cfg: &Config) -> Result<()> {
    if cfg.project.name.trim().is_empty() {
        bail!("project.name 不能为空");
    }
    if !cfg.project.name.chars().any(|c| c.is_alphabetic()) {
        bail!("project.name `{}` 必须包含字母", cfg.project.name);
    }
    if !matches!(cfg.project.cxx_std, 11 | 14 | 17 | 20 | 23 | 26) {
        bail!(
            "project.cxx_std = {} 不是合法的 C++ 标准（可选 11/14/17/20/23/26）",
            cfg.project.cxx_std
        );
    }

    let mut seen: HashMap<&str, &str> = HashMap::new();
    for t in &cfg.targets {
        if t.name.trim().is_empty() {
            bail!("存在名称为空的 target");
        }
        if let Some(prev) = seen.insert(t.name.as_str(), t.kind_name()) {
            bail!("target 名称重复：`{}`（已有一个 {}）", t.name, prev);
        }
        if t.sources.is_empty() {
            bail!(
                "target `{}` 没有源文件；如果是纯头文件的接口库，请先给它一个空的源文件占位",
                t.name
            );
        }
        if t.test && t.kind != TargetKind::Executable {
            bail!(
                "target `{}` 声明为测试，但类型是 {}；测试必须是可执行文件",
                t.name,
                t.kind
            );
        }
        if t.kind == TargetKind::Executable && !t.public_includes.is_empty() {
            // 可执行文件不是链接目标，PUBLIC 头文件目录无人继承，属于配置错误。
            bail!(
                "target `{}` 是可执行文件，没有 `public_includes` 的意义，请用 include_dirs",
                t.name
            );
        }
        if let Some(std) = t.cxx_std {
            if !matches!(std, 11 | 14 | 17 | 20 | 23 | 26) {
                bail!("target `{}` 的 cxx_std = {} 不合法", t.name, std);
            }
        }
    }

    for t in &cfg.targets {
        for d in &t.deps {
            if d == &t.name {
                bail!("target `{}` 依赖了它自己", t.name);
            }
            if !cfg.targets.iter().any(|x| &x.name == d) {
                bail!(
                    "target `{}` 依赖了不存在的 target `{}`；外部库请用 dependency() 声明后用 link_libraries 链接",
                    t.name,
                    d
                );
            }
        }
    }

    for d in &cfg.dependencies {
        if d.name.trim().is_empty() {
            bail!("存在名称为空的 dependency");
        }
    }

    for t in &cfg.targets {
        for step in &t.post_build {
            check_post_build(&t.name, step)?;
        }
    }

    Ok(())
}

fn check_post_build(target: &str, step: &PostBuildStep) -> Result<()> {
    match step {
        PostBuildStep::Command { program, .. } => {
            if program.trim().is_empty() {
                bail!("target `{target}` 的 post_build 命令缺少 program");
            }
        }
        PostBuildStep::Copy { sources, dest } => {
            if sources.is_empty() {
                bail!("target `{target}` 的 post_build 拷贝没有源文件");
            }
            if dest.trim().is_empty() {
                bail!("target `{target}` 的 post_build 拷贝缺少目标目录");
            }
            for s in sources {
                if s.trim().is_empty() {
                    bail!("target `{target}` 的 post_build 拷贝里有空的源路径");
                }
            }
        }
        PostBuildStep::CopyDir { src, dest } => {
            if src.trim().is_empty() || dest.trim().is_empty() {
                bail!("target `{target}` 的 post_build 目录拷贝缺少 src 或 dest");
            }
        }
        PostBuildStep::Symlink { src, dest, name } => {
            if src.trim().is_empty() || dest.trim().is_empty() {
                bail!("target `{target}` 的 post_build 符号链接缺少 src 或 dest");
            }
            if let Some(n) = name {
                if n.trim().is_empty() || n.contains('/') || n.contains('\\') {
                    bail!("target `{target}` 的符号链接名字 `{n}` 不合法：不能为空或含路径分隔符");
                }
            }
        }
    }
    Ok(())
}

/// 返回 target 下标序列：被依赖者一定排在依赖者之前。
pub fn order(cfg: &Config) -> Result<Vec<usize>> {
    let n = cfg.targets.len();
    let mut index: HashMap<&str, usize> = HashMap::with_capacity(n);
    for (i, t) in cfg.targets.iter().enumerate() {
        index.insert(t.name.as_str(), i);
    }

    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, t) in cfg.targets.iter().enumerate() {
        for d in &t.deps {
            match index.get(d.as_str()) {
                Some(&j) => edges[i].push(j),
                None => bail!("target `{}` 依赖的 `{}` 不存在", t.name, d),
            }
        }
    }

    let names: Vec<&str> = cfg.targets.iter().map(|t| t.name.as_str()).collect();
    let mut state = vec![Visit::New; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut out: Vec<usize> = Vec::with_capacity(n);

    fn visit(
        i: usize,
        names: &[&str],
        edges: &[Vec<usize>],
        state: &mut [Visit],
        stack: &mut Vec<usize>,
        out: &mut Vec<usize>,
    ) -> Result<()> {
        match state[i] {
            Visit::Done => return Ok(()),
            Visit::Active => {
                let start = stack.iter().position(|&x| x == i).unwrap_or(0);
                let mut path: Vec<&str> = stack[start..].iter().map(|&k| names[k]).collect();
                path.push(names[i]);
                bail!("检测到循环依赖：{}", path.join(" -> "));
            }
            Visit::New => {}
        }

        state[i] = Visit::Active;
        stack.push(i);
        for &j in &edges[i] {
            visit(j, names, edges, state, stack, out)?;
        }
        stack.pop();
        state[i] = Visit::Done;
        out.push(i);
        Ok(())
    }

    for i in 0..n {
        visit(i, &names, &edges, &mut state, &mut stack, &mut out)?;
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Active,
    Done,
}

impl crate::model::Target {
    fn kind_name(&self) -> &'static str {
        match self.kind {
            TargetKind::Executable => "可执行文件",
            TargetKind::StaticLibrary => "静态库",
            TargetKind::SharedLibrary => "动态库",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn cfg(targets: Vec<Target>) -> Config {
        Config {
            project: ProjectMeta {
                name: "t".into(),
                version: "0.1.0".into(),
                cxx_std: 20,
                description: None,
                build_dir: "build".into(),
            },
            targets,
            dependencies: vec![],
            toolchain: Toolchain::default(),
            cmake: CmakeSnippets::default(),
            mutate: MutateConfig::default(),
            source_root: None,
            config_file: None,
        }
    }

    fn target(name: &str, kind: TargetKind, deps: &[&str]) -> Target {
        Target {
            name: name.into(),
            kind,
            sources: vec!["a.cpp".into()],
            include_dirs: vec![],
            public_includes: vec![],
            definitions: vec![],
            compile_options: vec![],
            link_options: vec![],
            link_libraries: vec![],
            deps: deps.iter().map(|s| s.to_string()).collect(),
            post_build: vec![],
            test: false,
            cxx_std: None,
            output_name: None,
        }
    }

    #[test]
    fn orders_dependencies_first() {
        let c = cfg(vec![
            target("app", TargetKind::Executable, &["core"]),
            target("core", TargetKind::StaticLibrary, &["base"]),
            target("base", TargetKind::StaticLibrary, &[]),
        ]);
        let idx = order(&c).unwrap();
        let names: Vec<&str> = idx.iter().map(|&i| c.targets[i].name.as_str()).collect();
        assert_eq!(names, vec!["base", "core", "app"]);
    }

    #[test]
    fn detects_cycle() {
        let c = cfg(vec![
            target("a", TargetKind::StaticLibrary, &["b"]),
            target("b", TargetKind::StaticLibrary, &["a"]),
        ]);
        let err = order(&c).unwrap_err().to_string();
        assert!(err.contains("循环依赖"), "unexpected: {err}");
        assert!(err.contains("a") && err.contains("b"));
    }

    #[test]
    fn rejects_duplicate_names() {
        let c = cfg(vec![
            target("dup", TargetKind::StaticLibrary, &[]),
            target("dup", TargetKind::StaticLibrary, &[]),
        ]);
        let err = validate(&c).unwrap_err().to_string();
        assert!(err.contains("重复"), "unexpected: {err}");
    }

    #[test]
    fn rejects_unknown_dep() {
        let c = cfg(vec![target("app", TargetKind::Executable, &["ghost"])]);
        assert!(validate(&c).is_err());
    }

    #[test]
    fn rejects_public_includes_on_executable() {
        let mut t = target("app", TargetKind::Executable, &[]);
        t.public_includes = vec!["include".into()];
        assert!(validate(&cfg(vec![t])).is_err());
    }

    #[test]
    fn rejects_empty_sources() {
        let mut t = target("app", TargetKind::Executable, &[]);
        t.sources.clear();
        assert!(validate(&cfg(vec![t])).is_err());
    }
}
