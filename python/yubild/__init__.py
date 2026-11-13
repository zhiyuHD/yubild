"""Yubild 配置 DSL。

在项目根目录的 ``yubild.py`` 里这样写::

    import yubild

    yubild.project(name="hello", version="0.1.0", cxx_std=20)

    yubild.static_library(
        "core",
        sources=["core/src/*.cpp"],
        public_includes=["core/include"],
    )

    yubild.executable(
        "hello",
        sources=["app/main.cpp"],
        deps=["core"],
    )

随后执行 ``yubild build``：配置在子进程里求值成 JSON，交给 Rust 侧校验并生成 CMakeLists.txt。
"""

from ._state import (
    YubildError,
    cmake,
    cmd,
    copy,
    copy_dir,
    dependency,
    executable,
    mutate_exclude,
    project,
    shared_library,
    sources,
    static_library,
    symlink,
    test,
    toolchain,
    mutate_exclude,
)

__all__ = [
    "YubildError",
    "project",
    "executable",
    "static_library",
    "shared_library",
    "dependency",
    "toolchain",
    "sources",
    "cmake",
    "cmd",
    "copy",
    "copy_dir",
    "test",
    "symlink",
    "mutate_exclude",
]

__version__ = "0.1.0"
