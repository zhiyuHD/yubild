//! Python 桥：调用解释器执行用户的 `yubild.py`，拿回 JSON 模型。
//!
//! 用子进程而不是内嵌解释器（pyo3），原因是：Windows 上打包 Python 运行时的
//! 负担远大于一次子进程启动（约 30ms），而构建系统对启动延迟并不敏感。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::model::Config;

const RUNNER: &str = "from yubild._runner import main; main()";

/// 找到可用的 Python 解释器。
pub fn interpreter() -> Result<String> {
    if let Ok(p) = std::env::var("YUBILD_PYTHON") {
        if !p.trim().is_empty() {
            return Ok(p);
        }
    }

    let candidates: &[&str] = if cfg!(windows) {
        &["python", "python3", "py"]
    } else {
        &["python3", "python"]
    };

    for c in candidates {
        let ok = Command::new(c)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Ok((*c).to_string());
        }
    }

    bail!(
        "找不到 Python 解释器。yubild 用 Python 求值 yubild.py，请安装 python3，\
         或通过环境变量 YUBILD_PYTHON 指定解释器路径"
    );
}

fn is_python_home(p: &Path) -> bool {
    p.join("yubild").join("__init__.py").is_file()
}

fn canonical(p: PathBuf) -> PathBuf {
    p.canonicalize().unwrap_or(p)
}

/// 找到随 yubild 分发的 Python 包目录（包含 `yubild/__init__.py`）。
///
/// 查找顺序：`YUBILD_PYTHON_HOME` → 可执行文件旁边的安装布局 → 向上找源码树里的 `python/`。
/// 这里刻意不使用编译期路径：`env!("CARGO_MANIFEST_DIR")` 会把构建机的绝对路径
/// 烧进二进制，换台机器就失效。
pub fn home() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("YUBILD_PYTHON_HOME") {
        let p = PathBuf::from(p);
        if is_python_home(&p) {
            return Ok(canonical(p));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // 安装布局：<prefix>/bin/yubild 旁边的 python/
            for c in [
                dir.join("python"),
                dir.join("../python"),
                dir.join("../share/yubild/python"),
            ] {
                if is_python_home(&c) {
                    return Ok(canonical(c));
                }
            }

            // 开发布局：target/debug/yubild → 向上找到仓库根的 python/
            let mut cur = dir.to_path_buf();
            for _ in 0..6 {
                let candidate = cur.join("python");
                if is_python_home(&candidate) {
                    return Ok(canonical(candidate));
                }
                if !cur.pop() {
                    break;
                }
            }
        }
    }

    // 都找不到时（例如 `cargo install` 之后），从二进制里解出内置的 Python 包
    crate::assets::materialize()
}

/// 在子进程里求值配置，返回解析后的模型。
pub fn run(python: &str, home: &Path, config: &Path) -> Result<Config> {
    let out = Command::new(python)
        .arg("-c")
        .arg(RUNNER)
        .arg(config)
        .env("PYTHONPATH", home)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        // 关键：配置文件通常叫 yubild.py，若子进程以项目根为 cwd 启动，
        // `import yubild` 会先命中配置文件自己，形成自遮蔽。这里用一个中性目录启动，
        // 包目录只经由 PYTHONPATH 注入；项目根由 runner 在导入包之后再加入 sys.path。
        .current_dir(home)
        .output()
        .with_context(|| format!("无法启动 Python 解释器 `{python}`"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let code = out
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "信号".to_string());
        bail!(
            "yubild.py 求值失败（退出码 {code}）：\n{}",
            stderr.trim_end()
        );
    }

    // 用户脚本里的 print() 会被重定向到 stderr，正常情况下仍然值得展示。
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        for line in stderr.trim_end().lines() {
            eprintln!("  [python] {line}");
        }
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).with_context(|| {
        format!(
            "Python 侧返回的模型不是合法 JSON（stdout 前 500 字节）：\n{}",
            &stdout[..stdout.len().min(500)]
        )
    })
}
