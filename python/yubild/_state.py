"""Yubild 配置 DSL：状态持有与校验。

设计上刻意保持「薄」：这里只收集用户的声明并做即时校验，
真正的语义解析（排序、生成）在 Rust 侧完成，避免两处逻辑分叉。
"""

from __future__ import annotations

import glob as _glob
import os
import re

_NAME_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_-]*$")
_VERSION_RE = re.compile(r"^\d+(\.\d+){0,2}$")
_VALID_STD = (11, 14, 17, 20, 23, 26)
_VALID_KINDS = ("executable", "static_library", "shared_library")
_VALID_DEP_KINDS = ("find_package", "pkg_config", "git", "subdir")
_VALID_CMAKE_PHASES = ("before_project", "after_project")
_VALID_POST_BUILD_KINDS = ("command", "copy", "copy_dir", "symlink")
_DEST_ALIASES = {"exe_dir": "@exe_dir", "build_dir": "@build_dir", "source_dir": "@source_dir"}


class YubildError(ValueError):
    """配置书写错误，通常是用户笔误。"""


class _State:
    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.project = None
        self.targets: list[dict] = []
        self.dependencies: list[dict] = []
        self.toolchain: dict = {}
        self.cmake: dict[str, list[str]] = {"before_project": [], "after_project": []}
        self.mutate_exclude: list[str] = []


_state = _State()


# ------------------------------------------------------------------ 工具函数


def _norm(path: str) -> str:
    return os.path.normpath(path).replace(os.sep, "/")


def _as_list(values):
    if values is None:
        return []
    if isinstance(values, (str, os.PathLike)):
        return [values]
    return list(values)


def _expand(patterns) -> list[str]:
    """展开 glob、去重、统一成正斜杠的相对路径。"""
    out: list[str] = []
    for raw in _as_list(patterns):
        pat = os.fspath(raw)
        if any(ch in pat for ch in "*?["):
            matches = sorted(
                m for m in _glob.glob(pat, recursive=True) if os.path.isfile(m)
            )
            if not matches:
                raise YubildError(
                    f"通配符 {pat!r} 没有匹配到任何文件（工作目录: {os.getcwd()}）"
                )
            out.extend(matches)
        else:
            if not os.path.isfile(pat):
                raise YubildError(f"源文件不存在: {pat}（工作目录: {os.getcwd()}）")
            out.append(pat)

    seen: set[str] = set()
    result: list[str] = []
    for p in out:
        n = _norm(p)
        if n not in seen:
            seen.add(n)
            result.append(n)
    return result


def _dir_list(values, what: str) -> list[str]:
    return [_norm(os.fspath(v)) for v in _as_list(values)]


def _check_std(std) -> int | None:
    if std is None:
        return None
    if std not in _VALID_STD:
        raise YubildError(f"cxx_std 必须是 {_VALID_STD} 之一，收到 {std!r}")
    return int(std)


def _check_name(kind: str, name) -> str:
    if not isinstance(name, str) or not name.strip():
        raise YubildError(f"{kind} 名称必须是非空字符串，收到 {name!r}")
    if not _NAME_RE.match(name):
        raise YubildError(
            f"{kind} 名称 {name!r} 不合法：只能包含字母、数字、下划线和连字符，"
            "并且不能以数字开头（该名称会直接成为 CMake target 名）"
        )
    return name


# ------------------------------------------------------------------ 公开 API


def sources(*patterns) -> list[str]:
    """展开源文件列表，便于在多个 target 之间复用。"""
    return _expand(list(patterns))


def project(
    name: str,
    *,
    version: str = "0.1.0",
    cxx_std: int = 20,
    description: str | None = None,
    build_dir: str = "build",
) -> None:
    """声明项目元信息。必须且只能调用一次。"""
    if _state.project is not None:
        raise YubildError(
            f"project() 只能调用一次（已声明 {_state.project['name']!r}）"
        )
    if not isinstance(name, str) or not name.strip():
        raise YubildError(f"project 名称不能为空，收到 {name!r}")
    if not _VERSION_RE.match(str(version)):
        raise YubildError(
            f"version {version!r} 不合法：CMake 要求形如 1、1.2 或 1.2.3 的纯数字版本号"
        )
    if not build_dir.strip():
        raise YubildError("build_dir 不能为空")

    _state.project = {
        "name": name,
        "version": str(version),
        "cxx_std": _check_std(cxx_std) or 20,
        "description": description,
        "build_dir": _norm(build_dir),
    }


def _target(
    kind: str,
    name: str,
    *,
    sources,
    include_dirs=(),
    public_includes=(),
    definitions=(),
    compile_options=(),
    link_options=(),
    link_libraries=(),
    deps=(),
    post_build=(),
    cxx_std=None,
    output_name=None,
) -> dict:
    if kind not in _VALID_KINDS:
        raise YubildError(f"未知的 target 类型 {kind!r}")
    _check_name("target", name)

    public = _dir_list(public_includes, "public_includes")
    if kind == "executable" and public:
        raise YubildError(
            f"可执行文件 {name!r} 不是链接目标，不支持 public_includes；请改用 include_dirs"
        )

    expanded = _expand(sources)
    if not expanded:
        raise YubildError(f"target {name!r} 没有源文件")

    entry = {
        "name": name,
        "type": kind,
        "sources": expanded,
        "include_dirs": _dir_list(include_dirs, "include_dirs"),
        "public_includes": public,
        "definitions": [str(d) for d in _as_list(definitions)],
        "compile_options": [str(o) for o in _as_list(compile_options)],
        "link_options": [str(o) for o in _as_list(link_options)],
        "link_libraries": [str(l) for l in _as_list(link_libraries)],
        "deps": [],
        "post_build": _post_build_steps(name, post_build),
        "cxx_std": _check_std(cxx_std),
        "output_name": output_name,
    }

    for d in _as_list(deps):
        if not isinstance(d, str):
            raise YubildError(f"target {name!r} 的 deps 里有非字符串项 {d!r}")
        if d == name:
            raise YubildError(f"target {name!r} 依赖了它自己")
        entry["deps"].append(d)

    _state.targets.append(entry)
    return entry


def executable(name: str, *, sources, **kwargs) -> dict:
    """可执行文件。"""
    return _target("executable", name, sources=sources, **kwargs)


def static_library(name: str, *, sources, **kwargs) -> dict:
    """静态库（.a / .lib）。"""
    return _target("static_library", name, sources=sources, **kwargs)


def shared_library(name: str, *, sources, **kwargs) -> dict:
    """动态库（.so / .dll / .dylib）。"""
    return _target("shared_library", name, sources=sources, **kwargs)


def test(name: str, *, sources, **kwargs) -> dict:
    """声明一个测试可执行文件。

    yubild 会自动给这个 target 加上测试库和 main()，所以测试源文件里只需要
    写 ``YB_TEST(...)``，不用自己处理进程入口::

        yubild.test("core_tests", sources=["tests/*.cpp"], deps=["core"])
    """
    if "post_build" in kwargs:
        raise YubildError("测试 target 不支持 post_build")
    entry = _target("executable", name, sources=sources, **kwargs)
    if entry["public_includes"]:
        raise YubildError(f"测试 target {name!r} 不支持 public_includes")
    entry["test"] = True
    return entry


def symlink(src: str, *, dest: str = "exe_dir", name: str | None = None) -> dict:
    """POST_BUILD 时创建符号链接（开发期改文件不用重建）。

    发布用 ``copy``/``copy_dir``，日常开发用 ``symlink`` 指向源码目录::

        yubild.symlink("webui", dest="exe_dir")          # bin/webui -> <项目>/webui
        yubild.symlink("webui", dest="exe_dir", name="ui")
    """
    if not isinstance(src, str) or not src.strip():
        raise YubildError("symlink() 需要非空的源路径")
    if name is not None and (not isinstance(name, str) or not name.strip()):
        raise YubildError("symlink() 的 name 必须是非空字符串")
    return {
        "kind": "symlink",
        "src": src,
        "dest": _normalize_dest(dest),
        "name": name,
    }


def mutate_exclude(*patterns: str) -> None:
    """变异测试复制工作副本时跳过的路径（gitignore 风格）。

    仓库里常见几百 MB 的 vendor SDK，默认规则之外还要显式排除::

        yubild.mutate_exclude("vendor/**", "third_party/cef/**")
    """
    for p in patterns:
        if not isinstance(p, str) or not p.strip():
            raise YubildError(f"mutate_exclude 的模式必须是非空字符串，收到 {p!r}")
        _state.mutate_exclude.append(p.strip())


def dependency(
    name: str,
    *,
    kind: str = "find_package",
    package: str | None = None,
    link: str | None = None,
    required: bool = True,
    version: str | None = None,
    url: str | None = None,
    tag: str | None = None,
    subdir: str | None = None,
) -> dict:
    """声明一个外部依赖。

    ``kind="find_package"`` 会生成 ``find_package(<package> REQUIRED)``，
    随后在 target 的 ``link_libraries`` 里写 ``fmt::fmt`` 这类导入目标即可。
    """
    if kind not in _VALID_DEP_KINDS:
        raise YubildError(f"依赖 {name!r} 的 kind={kind!r} 不合法，可选 {_VALID_DEP_KINDS}")
    if not isinstance(name, str) or not name.strip():
        raise YubildError("依赖名称不能为空")
    if kind == "git" and not url:
        raise YubildError(f"依赖 {name!r} 使用 kind='git' 时必须提供 url")
    if kind == "subdir" and not subdir:
        subdir = name

    if link is None and kind == "pkg_config":
        var = "".join(c.upper() if c.isalnum() else "_" for c in (package or name))
        link = f"PkgConfig::{var}"

    entry = {
        "name": name,
        "kind": kind,
        "package": package,
        "link": link,
        "required": bool(required),
        "version": version,
        "url": url,
        "tag": tag,
        "subdir": subdir,
    }
    _state.dependencies.append(entry)
    return entry


def _normalize_dest(dest) -> str:
    if not isinstance(dest, str) or not dest.strip():
        raise YubildError("post_build 的目标目录必须是非空字符串")
    return _DEST_ALIASES.get(dest, dest)


def _post_build_steps(target: str, steps) -> list[dict]:
    out: list[dict] = []
    for step in _as_list(steps):
        if isinstance(step, dict) and step.get("kind") in _VALID_POST_BUILD_KINDS:
            out.append(step)
        else:
            raise YubildError(
                f"target {target!r} 的 post_build 里有无法识别的动作 {step!r}；"
                "请用 cmake.copy() / cmake.copy_dir() / cmake.cmd() 构造"
            )
    return out


def cmake(code, *, phase: str = "after_project") -> None:
    """注入一段原始 CMake 代码。

    ``phase="before_project"`` 用于必须在 project() 之前生效的设置
    （工具链、缓存变量）；默认的 ``"after_project"`` 用于依赖 ``find_package``、
    自定义函数等需要在编译器探测之后才成立的调用::

        yubild.cmake('set(CEF_ROOT "${CMAKE_SOURCE_DIR}/cef_binary")')
        yubild.cmake('list(APPEND CMAKE_MODULE_PATH "${CEF_ROOT}/cmake")')
        yubild.cmake("find_package(CEF REQUIRED)")
    """
    if phase not in _VALID_CMAKE_PHASES:
        raise YubildError(
            f"cmake() 的 phase 必须是 {_VALID_CMAKE_PHASES} 之一，收到 {phase!r}"
        )
    lines = [code] if isinstance(code, str) else list(code)
    text = "\n".join(str(line).rstrip() for line in lines).strip("\n")
    if not text.strip():
        raise YubildError("cmake() 片段不能为空")
    _state.cmake[phase].append(text)


def cmd(program: str, *args) -> dict:
    """POST_BUILD 里执行一条任意命令。"""
    if not isinstance(program, str) or not program.strip():
        raise YubildError(f"cmd() 需要非空的程序名，收到 {program!r}")
    return {"kind": "command", "program": program, "args": [str(a) for a in args]}


def copy(*sources, dest: str = "exe_dir") -> dict:
    """POST_BUILD 时把文件拷到目标目录（默认是可执行文件所在目录）。"""
    if not sources:
        raise YubildError("copy() 至少需要一个源文件")
    return {
        "kind": "copy",
        "sources": [str(s) for s in sources],
        "dest": _normalize_dest(dest),
    }


def copy_dir(src: str, *, dest: str = "exe_dir") -> dict:
    """POST_BUILD 时递归拷贝一个目录（如 resources/、locales/）。"""
    if not isinstance(src, str) or not src.strip():
        raise YubildError("copy_dir() 需要非空的源目录")
    return {"kind": "copy_dir", "src": src, "dest": _normalize_dest(dest)}


def _infer_system(name: str) -> str | None:
    low = name.lower()
    if "mingw" in low or "windows" in low or "msvc" in low:
        return "Windows"
    if "android" in low:
        return "Android"
    if "linux" in low:
        return "Linux"
    if "darwin" in low or "apple" in low or "macos" in low:
        return "Darwin"
    return None


def toolchain(
    *,
    cross_prefix: str | None = None,
    system_name: str | None = None,
    generator: str | None = None,
    cxx_compiler: str | None = None,
    c_compiler: str | None = None,
    static_runtime: bool | None = None,
) -> None:
    """声明工具链/交叉编译设置。

    ``cross_prefix="x86_64-w64-mingw32-"`` 会拼出交叉编译器名，
    并据此推断 ``CMAKE_SYSTEM_NAME=Windows``（可显式覆盖）。
    """
    if cross_prefix is not None and system_name is None:
        system_name = _infer_system(cross_prefix)
    if system_name is None and cxx_compiler is not None:
        system_name = _infer_system(cxx_compiler)

    updates = {
        "cross_prefix": cross_prefix,
        "system_name": system_name,
        "generator": generator,
        "cxx_compiler": cxx_compiler,
        "c_compiler": c_compiler,
        "static_runtime": static_runtime,
    }
    for key, value in updates.items():
        if value is not None:
            _state.toolchain[key] = value


# ------------------------------------------------------------------ 导出


def export() -> dict:
    """把收集到的声明序列化成 Rust 侧消费的模型。"""
    if _state.project is None:
        raise YubildError("yubild.py 里没有调用 yubild.project(...)")
    if not _state.targets:
        raise YubildError(
            "项目里没有任何 target；用 executable()/static_library()/shared_library() 定义"
        )

    names = [t["name"] for t in _state.targets]
    duplicated = sorted({n for n in names if names.count(n) > 1})
    if duplicated:
        raise YubildError(f"target 名称重复: {', '.join(duplicated)}")

    known = set(names)
    for t in _state.targets:
        for d in t["deps"]:
            if d not in known:
                raise YubildError(
                    f"target {t['name']!r} 依赖了未定义的 target {d!r}；"
                    "外部库请用 dependency() 声明后通过 link_libraries 链接"
                )

    return {
        "project": dict(_state.project),
        "targets": list(_state.targets),
        "dependencies": list(_state.dependencies),
        "toolchain": dict(_state.toolchain),
        "cmake": {phase: list(items) for phase, items in _state.cmake.items()},
        "mutate": {"exclude": list(_state.mutate_exclude)},
    }
