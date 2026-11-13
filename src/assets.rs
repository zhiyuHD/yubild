//! 随二进制分发的 Python 配置层。
//!
//! `yubild.py` 需要 `import yubild`，所以解释器必须能找到这个包。
//! 开发态可以直接用源码树里的 `python/`；`cargo install` 之后源码树不存在了，
//! 于是把三个文件嵌进二进制，首次运行时释放到缓存目录。
//!
//! 目录名带版本号：升级 yubild 不会和旧版本残留的包互相污染。

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

const PY_INIT: &str = include_str!("../python/yubild/__init__.py");
const PY_STATE: &str = include_str!("../python/yubild/_state.py");
const PY_RUNNER: &str = include_str!("../python/yubild/_runner.py");

/// 把内置的 Python 包释放到缓存目录，返回可作为 `PYTHONPATH` 的目录。
pub fn materialize() -> Result<PathBuf> {
    let base = cache_root()?
        .join("yubild")
        .join(format!("python-{}", env!("CARGO_PKG_VERSION")));
    let pkg = base.join("yubild");
    fs::create_dir_all(&pkg).with_context(|| format!("无法创建缓存目录 {}", pkg.display()))?;

    for (name, content) in [
        ("__init__.py", PY_INIT),
        ("_state.py", PY_STATE),
        ("_runner.py", PY_RUNNER),
    ] {
        let path = pkg.join(name);
        // 内容一致就不重写，避免每次运行都碰磁盘
        if fs::read_to_string(&path)
            .map(|existing| existing == content)
            .unwrap_or(false)
        {
            continue;
        }
        fs::write(&path, content).with_context(|| format!("无法写入 {}", path.display()))?;
    }

    Ok(base)
}

/// 缓存根目录：`YUBILD_CACHE_DIR` 优先，其次是各平台惯例位置。
fn cache_root() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("YUBILD_CACHE_DIR") {
        if !p.trim().is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if cfg!(windows) {
        if let Ok(p) = std::env::var("LOCALAPPDATA") {
            if !p.trim().is_empty() {
                return Ok(PathBuf::from(p));
            }
        }
    }
    if let Ok(p) = std::env::var("XDG_CACHE_HOME") {
        if !p.trim().is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Ok(PathBuf::from(home).join(".cache"));
        }
    }
    if let Ok(p) = std::env::var("APPDATA") {
        if !p.trim().is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    bail!("无法确定缓存目录；请设置环境变量 YUBILD_CACHE_DIR")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_embedded_package() {
        let tmp = std::env::temp_dir().join(format!("yubild-assets-{}", std::process::id()));
        std::env::set_var("YUBILD_CACHE_DIR", &tmp);
        let home = materialize().expect("释放内置包");
        assert!(home.join("yubild").join("_state.py").is_file());
        assert!(home.join("yubild").join("_runner.py").is_file());
        // 再执行一次应命中缓存，不报错
        let again = materialize().expect("命中缓存");
        assert_eq!(home, again);
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("YUBILD_CACHE_DIR");
    }
}
