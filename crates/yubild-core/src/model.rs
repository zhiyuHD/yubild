//! 项目模型：Yubild 配置在 Python 侧求值后，经过 JSON 协议传到这里。

use std::fmt;

use serde::{Deserialize, Serialize};

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_cxx_std() -> u32 {
    20
}

fn default_build_dir() -> String {
    "build".to_string()
}

fn default_true() -> bool {
    true
}

fn default_dep_kind() -> DepKind {
    DepKind::FindPackage
}

/// 一份完整的项目配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectMeta,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub toolchain: Toolchain,
    /// 用户注入的原始 CMake 片段。
    #[serde(default)]
    pub cmake: CmakeSnippets,
    /// 变异测试的额外配置。
    #[serde(default)]
    pub mutate: MutateConfig,
    /// 项目根目录（`yubild.py` 所在目录），由 Python 侧填写。
    #[serde(default)]
    pub source_root: Option<String>,
    /// 配置文件路径，仅用于诊断信息。
    #[serde(default)]
    pub config_file: Option<String>,
}

/// 原始 CMake 片段。分两个落点是因为 CMake 的求值顺序是硬约束：
/// 工具链相关变量必须在 `project()` 之前设置，而 `find_package(CEF)` 这类
/// 需要编译器探测结果的调用必须在 `project()` 之后。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CmakeSnippets {
    #[serde(default)]
    pub before_project: Vec<String>,
    #[serde(default)]
    pub after_project: Vec<String>,
}

/// 变异测试配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MutateConfig {
    /// 复制工作副本时跳过的路径（gitignore 风格通配，如 `vendor/**`）。
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// POST_BUILD 动作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PostBuildStep {
    /// 任意命令：`COMMAND <program> <args...>`
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// 拷贝单个或多个文件到目标目录。
    Copy {
        #[serde(default)]
        sources: Vec<String>,
        dest: String,
    },
    /// 递归拷贝目录。
    CopyDir { src: String, dest: String },
    /// 创建符号链接，开发期改完文件刷新即可，不必重建。
    Symlink {
        src: String,
        dest: String,
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_cxx_std")]
    pub cxx_std: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_build_dir")]
    pub build_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Executable,
    StaticLibrary,
    SharedLibrary,
}

impl TargetKind {
    pub fn is_library(self) -> bool {
        matches!(self, Self::StaticLibrary | Self::SharedLibrary)
    }

    /// CMake `add_library` 的类型参数；可执行文件没有。
    pub fn cmake_type(self) -> Option<&'static str> {
        match self {
            Self::Executable => None,
            Self::StaticLibrary => Some("STATIC"),
            Self::SharedLibrary => Some("SHARED"),
        }
    }
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Executable => "executable",
            Self::StaticLibrary => "static_library",
            Self::SharedLibrary => "shared_library",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: TargetKind,
    #[serde(default)]
    pub sources: Vec<String>,
    /// 仅本 target 使用的头文件目录（PRIVATE）。
    #[serde(default)]
    pub include_dirs: Vec<String>,
    /// 会被依赖方继承的头文件目录（PUBLIC）。
    #[serde(default)]
    pub public_includes: Vec<String>,
    #[serde(default)]
    pub definitions: Vec<String>,
    #[serde(default)]
    pub compile_options: Vec<String>,
    #[serde(default)]
    pub link_options: Vec<String>,
    /// 原样传给 target_link_libraries 的链接项（可含 `fmt::fmt` 这类命名空间目标）。
    #[serde(default)]
    pub link_libraries: Vec<String>,
    /// 指向本项目中其它 target 的依赖。
    #[serde(default)]
    pub deps: Vec<String>,
    /// 链接/构建完成后执行的动作，典型用途是把运行时资源摆到可执行文件旁边。
    #[serde(default)]
    pub post_build: Vec<PostBuildStep>,
    /// 是否是一个测试可执行文件（`yubild test` 会构建并运行它）。
    #[serde(default)]
    pub test: bool,
    #[serde(default)]
    pub cxx_std: Option<u32>,
    #[serde(default)]
    pub output_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepKind {
    /// CMake 的 `find_package`
    FindPackage,
    /// pkg-config
    PkgConfig,
    /// FetchContent（配置期克隆）
    Git,
    /// `add_subdirectory` 引入的子项目
    Subdir,
}

impl DepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FindPackage => "find_package",
            Self::PkgConfig => "pkg_config",
            Self::Git => "git",
            Self::Subdir => "subdir",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    #[serde(default = "default_dep_kind")]
    pub kind: DepKind,
    /// 实际查找的包名，缺省用 `name`。
    #[serde(default)]
    pub package: Option<String>,
    /// 建议的链接目标，仅作提示用途。
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub subdir: Option<String>,
}

impl Dependency {
    pub fn package_name(&self) -> &str {
        self.package.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Toolchain {
    /// 例如 `x86_64-w64-mingw32-`，会拼出 `x86_64-w64-mingw32-g++`。
    #[serde(default)]
    pub cross_prefix: Option<String>,
    /// 例如 `Windows`、`Linux`、`Android`。
    #[serde(default)]
    pub system_name: Option<String>,
    #[serde(default)]
    pub generator: Option<String>,
    #[serde(default)]
    pub cxx_compiler: Option<String>,
    #[serde(default)]
    pub c_compiler: Option<String>,
    /// 静态链接 MSVC 运行时。
    #[serde(default)]
    pub static_runtime: Option<bool>,
}

impl Toolchain {
    pub fn is_cross(&self) -> bool {
        self.system_name.is_some() || self.cross_prefix.is_some() || self.cxx_compiler.is_some()
    }

    pub fn resolved_cxx(&self) -> Option<String> {
        self.cxx_compiler
            .clone()
            .or_else(|| self.cross_prefix.as_ref().map(|p| format!("{p}g++")))
    }

    pub fn resolved_c(&self) -> Option<String> {
        self.c_compiler
            .clone()
            .or_else(|| self.cross_prefix.as_ref().map(|p| format!("{p}gcc")))
    }
}
