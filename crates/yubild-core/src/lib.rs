//! Yubild 的配置模型、校验与依赖解析。
//!
//! 这一层不关心 CMake，也不关心 Python：它只处理「已经求值完毕的 JSON」。

pub mod model;
pub mod resolve;

pub use model::{
    CmakeSnippets, Config, DepKind, Dependency, MutateConfig, PostBuildStep, ProjectMeta, Target,
    TargetKind, Toolchain,
};
pub use resolve::{order, validate};

/// 校验 + 排序一步到位。
pub fn resolve(cfg: &Config) -> anyhow::Result<Vec<usize>> {
    validate(cfg)?;
    order(cfg)
}
