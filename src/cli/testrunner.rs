//! 测试运行器：执行测试可执行文件、解析协议、实时渲染结果。

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;

pub const PROTOCOL_PREFIX: &str = "@@YUBILD@@ ";

/// ANSI 着色。非 TTY、设置了 NO_COLOR 或显式 `--no-color` 时自动降级为纯文本。
pub struct Painter {
    enabled: bool,
}

impl Painter {
    pub fn new(force_off: bool) -> Self {
        let enabled =
            !force_off && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Self { enabled }
    }

    fn wrap(&self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    pub fn green(&self, s: &str) -> String {
        self.wrap("32", s)
    }

    pub fn red(&self, s: &str) -> String {
        self.wrap("31", s)
    }

    pub fn yellow(&self, s: &str) -> String {
        self.wrap("33", s)
    }

    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }

    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
}

#[derive(Debug, Default)]
pub struct CaseResult {
    pub name: String,
    pub ok: bool,
    pub ms: f64,
    pub file: String,
    pub line: u64,
    pub expr: String,
    pub detail: String,
    pub output: Vec<String>,
}

#[derive(Debug, Default)]
pub struct SuiteResult {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub elapsed: Duration,
    pub timed_out: bool,
    pub crashed: Option<String>,
    pub cases: Vec<CaseResult>,
}

/// 执行一个测试可执行文件，边跑边渲染。
pub fn run_suite(
    exe: &Path,
    root: &Path,
    filter: Option<&str>,
    p: &Painter,
    verbose: bool,
    quiet: bool,
    timeout: Duration,
) -> Result<SuiteResult> {
    let mut cmd = Command::new(exe);
    if let Some(f) = filter {
        if !f.is_empty() {
            cmd.arg(format!("--filter={f}"));
        }
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child: Child = cmd
        .spawn()
        .with_context(|| format!("无法运行测试可执行文件 {}", exe.display()))?;

    let stdout = child.stdout.take().expect("stdout 已被设为 piped");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let render = Render {
        p,
        root,
        verbose,
        quiet,
    };

    let started = Instant::now();
    let deadline = started + timeout;
    let mut suite = SuiteResult::default();
    let mut current: Option<CaseResult> = None;
    let mut seen_suite_end = false;

    loop {
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            suite.timed_out = true;
            if !quiet {
                println!();
                println!(
                    "  {}",
                    p.red(&format!("超时：{} 秒内没有跑完", timeout.as_secs()))
                );
            }
            break;
        }

        match rx.recv_timeout(deadline - now) {
            Ok(line) => {
                if let Some(json) = line.strip_prefix(PROTOCOL_PREFIX) {
                    if let Ok(v) = serde_json::from_str::<Value>(json) {
                        handle_event(&v, &render, &mut suite, &mut current, &mut seen_suite_end);
                        continue;
                    }
                }
                // 非协议输出：挂在当前用例上，失败时回显
                match current.as_mut() {
                    Some(c) => c.output.push(line),
                    None => {
                        if verbose && !quiet {
                            println!("  {}", p.dim(&format!("| {line}")));
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait().ok();
    suite.elapsed = started.elapsed();

    if let Some(st) = status {
        if !seen_suite_end {
            let how = st
                .code()
                .map(|c| format!("退出码 {c}"))
                .unwrap_or_else(|| "被信号终止".to_string());
            suite.crashed = Some(how.clone());
            if !quiet {
                println!(
                    "  {}",
                    p.red(&format!("测试进程异常结束（{how}），上面的用例可能崩溃了"))
                );
            }
        }
    }

    Ok(suite)
}

/// 渲染所需的共享上下文（参数多了就收进结构体，避免函数签名膨胀）。
struct Render<'a> {
    p: &'a Painter,
    root: &'a Path,
    verbose: bool,
    quiet: bool,
}

fn handle_event(
    v: &Value,
    r: &Render,
    suite: &mut SuiteResult,
    current: &mut Option<CaseResult>,
    seen_suite_end: &mut bool,
) {
    let (p, root, verbose, quiet) = (r.p, r.root, r.verbose, r.quiet);
    match v["event"].as_str().unwrap_or_default() {
        "suite_start" => {
            suite.total = v["total"].as_u64().unwrap_or(0) as usize;
            if !quiet {
                println!("{}", p.dim(&format!("共 {} 个用例", suite.total)));
            }
        }
        "case_start" => {
            let idx = v["index"].as_u64().unwrap_or(0);
            let total = v["total"].as_u64().unwrap_or(0);
            let name = v["name"].as_str().unwrap_or("").to_string();
            if !quiet {
                print!(
                    "  {} {} ",
                    p.dim(&format!("[{idx}/{total}]")),
                    p.bold(&name)
                );
                let _ = std::io::stdout().flush();
            }
            *current = Some(CaseResult {
                name,
                ..Default::default()
            });
        }
        "case_end" => {
            let mut case = current.take().unwrap_or_default();
            case.ok = v["ok"].as_bool().unwrap_or(false);
            case.ms = v["ms"].as_f64().unwrap_or(0.0);
            case.file = rel(root, v["file"].as_str().unwrap_or(""));
            case.line = v["line"].as_u64().unwrap_or(0);
            case.expr = v["expr"].as_str().unwrap_or("").to_string();
            case.detail = v["detail"].as_str().unwrap_or("").to_string();

            if !quiet {
                if case.ok {
                    println!("{} {}", p.green("✓"), p.dim(&format!("{:.3}ms", case.ms)));
                } else {
                    println!("{}", p.red("✗"));
                }
            }

            if !case.ok {
                if !case.file.is_empty() && case.line > 0 {
                    println!("      {} {}:{}", p.dim("位置"), case.file, case.line);
                } else if !case.file.is_empty() {
                    println!("      {} {}", p.dim("位置"), case.file);
                }
                if !case.expr.is_empty() {
                    println!("      {} {}", p.dim("断言"), case.expr);
                }
                if !case.detail.is_empty() {
                    println!("      {} {}", p.dim("实际"), case.detail);
                }
                for line in &case.output {
                    println!("      {}", p.dim(line));
                }
            } else if verbose && !quiet {
                for line in &case.output {
                    println!("      {}", p.dim(line));
                }
            }

            if case.ok {
                suite.passed += 1;
            } else {
                suite.failed += 1;
            }
            suite.cases.push(case);
        }
        "suite_end" => {
            *seen_suite_end = true;
            suite.passed = v["passed"].as_u64().unwrap_or(suite.passed as u64) as usize;
            suite.failed = v["failed"].as_u64().unwrap_or(suite.failed as u64) as usize;
        }
        _ => {}
    }
}

/// 把 `__FILE__` 的绝对路径缩成项目内相对路径。
fn rel(root: &Path, file: &str) -> String {
    if file.is_empty() {
        return String::new();
    }
    match Path::new(file).strip_prefix(root) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => file.to_string(),
    }
}

/// `--list`：列出测试可执行文件里的用例名。
pub fn list_cases(exe: &Path, filter: Option<&str>) -> Result<Vec<String>> {
    let mut cmd = Command::new(exe);
    cmd.arg("--list");
    if let Some(f) = filter {
        if !f.is_empty() {
            cmd.arg(format!("--filter={f}"));
        }
    }
    let out = cmd
        .output()
        .with_context(|| format!("无法运行 {}", exe.display()))?;
    if !out.status.success() {
        anyhow::bail!("{} --list 失败", exe.display());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}
