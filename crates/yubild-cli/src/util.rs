//! 变异测试与运行器共用的小工具。

use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// 通配匹配。
///
/// 语义刻意贴近 .gitignore：模式里没有 `/` 时只匹配文件名（任意深度）；
/// 含 `/` 时逐段匹配，`*` 不跨段，`**` 跨段。
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('/') {
        let base = text.rsplit('/').next().unwrap_or(text);
        return seg_match(pattern, base);
    }
    let pat: Vec<&str> = pattern.split('/').collect();
    let txt: Vec<&str> = text.split('/').collect();
    match_segments(&pat, &txt)
}

fn match_segments(pat: &[&str], txt: &[&str]) -> bool {
    if pat.is_empty() {
        return txt.is_empty();
    }
    if pat[0] == "**" {
        for i in 0..=txt.len() {
            if match_segments(&pat[1..], &txt[i..]) {
                return true;
            }
        }
        return false;
    }
    if txt.is_empty() {
        return false;
    }
    seg_match(pat[0], txt[0]) && match_segments(&pat[1..], &txt[1..])
}

fn seg_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[derive(Debug, Default)]
pub struct CopyStats {
    pub files: u64,
    pub linked: u64,
    pub copied: u64,
    pub skipped: u64,
}

/// 递归复制目录到工作副本。
///
/// 两件事很关键：
/// 1. **优先硬链接**：几 GB 的 vendor 目录如果真拷贝，一轮变异测试光准备就要几十秒；
///    硬链接只建目录项，几乎不占空间也不搬数据。
/// 2. **通配排除**：通过 `skip_globs` 跳过 `vendor/**` 这类目录。
///
/// 硬链接意味着同一个 inode 被两个路径引用，因此后续改写必须用
/// [`write_replacing`]（先写临时文件再 rename），否则会把用户的源码一起改掉。
pub fn copy_tree_filtered(
    src: &Path,
    dst: &Path,
    skip_names: &[&str],
    skip_globs: &[&str],
) -> Result<CopyStats> {
    let mut stats = CopyStats::default();
    copy_dir_rec(src, dst, "", skip_names, skip_globs, &mut stats)?;
    Ok(stats)
}

fn copy_dir_rec(
    src: &Path,
    dst: &Path,
    rel: &str,
    skip_names: &[&str],
    skip_globs: &[&str],
    stats: &mut CopyStats,
) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("无法创建 {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("无法读取 {}", src.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        let child_rel = if rel.is_empty() {
            name_str.clone()
        } else {
            format!("{rel}/{name_str}")
        };

        if skip_names.iter().any(|s| *s == name_str)
            || skip_globs
                .iter()
                .any(|g| wildcard_match(g, &child_rel) || wildcard_match(g, &name_str))
        {
            stats.skipped += 1;
            continue;
        }

        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_rec(&from, &to, &child_rel, skip_names, skip_globs, stats)?;
        } else if ft.is_file() {
            stats.files += 1;
            match fs::hard_link(&from, &to) {
                Ok(()) => stats.linked += 1,
                Err(_) => {
                    fs::copy(&from, &to).with_context(|| {
                        format!("无法复制 {} -> {}", from.display(), to.display())
                    })?;
                    stats.copied += 1;
                }
            }
        }
    }
    Ok(())
}

/// 原子替换文件内容。
///
/// 先写临时文件再 rename：replace 只换目录项，不会修改被硬链接共享的 inode，
/// 因此不会污染用户的原始源码树。
pub fn write_replacing(path: &Path, content: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let tmp = path.with_file_name(format!(".{file_name}.yubild-tmp"));
    fs::write(&tmp, content).with_context(|| format!("无法写入 {}", tmp.display()))?;

    #[cfg(windows)]
    {
        // Windows 上 rename 不能覆盖已存在的目标
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).with_context(|| format!("无法替换 {}", path.display()))?;
    Ok(())
}

pub enum Outcome {
    Exited(ExitStatus),
    TimedOut,
}

/// 运行子进程并限时；超时会杀掉进程。
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Outcome> {
    let mut child = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("无法启动子进程")?;

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Outcome::Exited(status));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Outcome::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::wildcard_match;

    #[test]
    fn wildcard_basics() {
        assert!(wildcard_match("*.cpp", "src/math.cpp"));
        assert!(wildcard_match("src/*.cpp", "src/math.cpp"));
        assert!(!wildcard_match("src/*.cpp", "src/sub/math.cpp"));
        assert!(wildcard_match("src/**/*.cpp", "src/sub/math.cpp"));
        assert!(wildcard_match("a?c", "abc"));
        assert!(!wildcard_match("a?c", "ac"));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("math.cpp", "math.cpp"));
        assert!(!wildcard_match("math.cpp", "math2.cpp"));
    }
}
