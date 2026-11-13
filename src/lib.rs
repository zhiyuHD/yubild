//! Yubild：用 Python 写配置的 C++ 构建系统。
//!
//! 分层：
//! - [`model`]：从 Python 求值结果反序列化出来的项目模型；
//! - [`resolve`]：校验与依赖排序；
//! - [`cmake`]：把模型翻译成 `CMakeLists.txt`；
//! - [`cli`]：命令行、子进程桥接、测试运行器、变异测试；
//! - [`assets`]：随二进制分发的 Python 配置层。

pub mod assets;
pub mod cli;
pub mod cmake;
pub mod model;
pub mod resolve;

pub use model::{
    CmakeSnippets, Config, DepKind, Dependency, MutateConfig, PostBuildStep, ProjectMeta, Target,
    TargetKind, Toolchain,
};
pub use resolve::{order, validate};

/// 校验 + 排序一步到位：返回值是被依赖者排在前面的 target 下标序列。
pub fn resolve_targets(cfg: &Config) -> anyhow::Result<Vec<usize>> {
    validate(cfg)?;
    order(cfg)
}
