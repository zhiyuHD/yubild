//! 变异体的编译预检。
//!
//! 文本变异必然产生一些编译不过的变异体（比如把 `a * b` 变成 `a / b` 有时类型不合法）。
//! 与其花几十秒重建整个 target 才发现，不如先用项目自己的编译参数跑一次
//! `-fsyntax-only`：只解析 + 语义检查，不生成目标文件、不链接，通常几十毫秒。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CompileDb {
    by_file: HashMap<PathBuf, Vec<String>>,
    cwd: PathBuf,
}

impl CompileDb {
    /// 从构建目录读取 `compile_commands.json`；不存在时返回 None（预检降级为跳过）。
    pub fn load(build_dir: &Path) -> Option<CompileDb> {
        let path = build_dir.join("compile_commands.json");
        let text = std::fs::read_to_string(&path).ok()?;
        let entries: Vec<serde_json::Value> = serde_json::from_str(&text).ok()?;

        let mut by_file: HashMap<PathBuf, Vec<String>> = HashMap::new();
        for entry in entries {
            let Some(file) = entry.get("file").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(argv) = argv_of(&entry) else {
                continue;
            };

            let mut p = PathBuf::from(file);
            if p.is_relative() {
                let dir = entry
                    .get("directory")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| build_dir.to_path_buf());
                p = dir.join(p);
            }
            by_file.entry(p).or_insert(argv);
        }

        if by_file.is_empty() {
            return None;
        }
        Some(CompileDb {
            by_file,
            cwd: build_dir.to_path_buf(),
        })
    }

    /// 检查一份源文件能否通过语法/语义检查。`None` 表示没有对应的编译命令。
    pub fn syntax_check(&self, file: &Path) -> Option<bool> {
        let argv = self
            .by_file
            .get(file)
            .or_else(|| file.canonicalize().ok().and_then(|c| self.by_file.get(&c)))?;

        let prepared = prepare_syntax_only(argv);
        if prepared.is_empty() {
            return None;
        }
        let status = Command::new(&prepared[0])
            .args(&prepared[1..])
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        Some(status.success())
    }
}

/// 取出条目里的命令行：优先 `arguments`，否则拆 `command`。
fn argv_of(entry: &serde_json::Value) -> Option<Vec<String>> {
    if let Some(args) = entry.get("arguments").and_then(|v| v.as_array()) {
        let v: Vec<String> = args
            .iter()
            .filter_map(|a| a.as_str().map(|s| s.to_string()))
            .collect();
        return if v.is_empty() { None } else { Some(v) };
    }
    let cmd = entry.get("command").and_then(|v| v.as_str())?;
    let v = split_command(cmd);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// 把编译命令改成「只检查语法」：去掉 `-c`、`-o`、依赖文件相关参数，加上 `-fsyntax-only`。
fn prepare_syntax_only(argv: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(argv.len() + 1);
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "-c" | "-fsyntax-only" => {
                i += 1;
            }
            "-o" | "-MT" | "-MF" => {
                i += 2;
            }
            "-MD" | "-MMD" | "-MP" => {
                i += 1;
            }
            _ => {
                out.push(argv[i].clone());
                i += 1;
            }
        }
    }
    out.push("-fsyntax-only".to_string());
    out
}

/// 拆分 shell 风格的命令串（compile_commands 的 `command` 字段）。
fn split_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in cmd.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        match quote {
            Some(q) => {
                if ch == '\\' && q == '"' {
                    escaped = true;
                } else if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                c => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_command() {
        let v = split_command("/usr/bin/c++ -I\"/path with space\" -c a.cpp -o a.o");
        assert_eq!(v[0], "/usr/bin/c++");
        assert!(v.contains(&"-I/path with space".to_string()));
        assert!(v.contains(&"a.o".to_string()));
    }

    #[test]
    fn strips_output_and_compile_only() {
        let argv: Vec<String> = [
            "c++",
            "-c",
            "a.cpp",
            "-o",
            "a.o",
            "-MD",
            "-MT",
            "x.o",
            "-MF",
            "x.d",
            "-std=c++20",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let out = prepare_syntax_only(&argv);
        assert!(out.contains(&"-fsyntax-only".to_string()));
        assert!(!out.contains(&"-c".to_string()));
        assert!(!out.contains(&"a.o".to_string()));
        assert!(!out.contains(&"-MF".to_string()));
        assert!(out.contains(&"-std=c++20".to_string()));
    }
}
