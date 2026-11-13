"""Yubild Python DSL 的单元测试（不依赖 cmake）。

运行: python3 -m unittest discover -s tests -p 'test_*.py'
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "python"))

import yubild  # noqa: E402
from yubild import _state  # noqa: E402


class DslTest(unittest.TestCase):
    def setUp(self) -> None:
        _state._state.reset()
        self._tmp = tempfile.TemporaryDirectory()
        self._old_cwd = os.getcwd()
        os.chdir(self._tmp.name)
        for name in ("a.cpp", "b.cpp"):
            open(name, "w").close()

    def tearDown(self) -> None:
        os.chdir(self._old_cwd)
        self._tmp.cleanup()

    def test_minimal_project_exports(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        cfg = _state.export()
        self.assertEqual(cfg["project"]["name"], "demo")
        self.assertEqual(cfg["project"]["cxx_std"], 20)
        self.assertEqual(len(cfg["targets"]), 1)
        self.assertEqual(cfg["targets"][0]["type"], "executable")
        self.assertEqual(cfg["targets"][0]["sources"], ["a.cpp"])

    def test_glob_expands_and_dedupes(self) -> None:
        yubild.project(name="demo")
        yubild.static_library("lib", sources=["*.cpp", "a.cpp"])
        sources = _state.export()["targets"][0]["sources"]
        self.assertEqual(sources, ["a.cpp", "b.cpp"])

    def test_project_is_singleton(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.project(name="other")

    def test_missing_source_is_rejected(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.executable("app", sources=["ghost.cpp"])

    def test_empty_glob_is_rejected(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.executable("app", sources=["*.zzz"])

    def test_public_includes_on_executable_is_rejected(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.executable("app", sources=["a.cpp"], public_includes=["inc"])

    def test_unknown_dep_is_rejected_at_export(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"], deps=["ghost"])
        with self.assertRaises(yubild.YubildError):
            _state.export()

    def test_duplicate_target_names_rejected(self) -> None:
        yubild.project(name="demo")
        yubild.static_library("dup", sources=["a.cpp"])
        yubild.static_library("dup", sources=["b.cpp"])
        with self.assertRaises(yubild.YubildError):
            _state.export()

    def test_bad_cxx_std_rejected(self) -> None:
        with self.assertRaises(yubild.YubildError):
            yubild.project(name="demo", cxx_std=19)

    def test_bad_version_rejected(self) -> None:
        with self.assertRaises(yubild.YubildError):
            yubild.project(name="demo", version="v1.0")

    def test_toolchain_infers_windows(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        yubild.toolchain(cross_prefix="x86_64-w64-mingw32-")
        tc = _state.export()["toolchain"]
        self.assertEqual(tc["system_name"], "Windows")
        self.assertEqual(tc["cross_prefix"], "x86_64-w64-mingw32-")

    def test_pkg_config_link_is_synthesised(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        yubild.dependency("libfmt", kind="pkg_config")
        dep = _state.export()["dependencies"][0]
        self.assertEqual(dep["link"], "PkgConfig::LIBFMT")

    def test_git_dependency_requires_url(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.dependency("fmt", kind="git")

    def test_sources_helper(self) -> None:
        self.assertEqual(yubild.sources("*.cpp"), ["a.cpp", "b.cpp"])


    def test_cmake_snippets_kept_per_phase(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        yubild.cmake("set(A 1)")
        yubild.cmake(["set(B 2)", "set(C 3)"])
        yubild.cmake("set(TOOLCHAIN 1)", phase="before_project")
        cfg = _state.export()
        self.assertEqual(cfg["cmake"]["before_project"], ["set(TOOLCHAIN 1)"])
        self.assertEqual(cfg["cmake"]["after_project"], ["set(A 1)", "set(B 2)\nset(C 3)"])

    def test_cmake_rejects_bad_phase(self) -> None:
        with self.assertRaises(yubild.YubildError):
            yubild.cmake("set(A 1)", phase="whenever")

    def test_cmake_rejects_empty_snippet(self) -> None:
        with self.assertRaises(yubild.YubildError):
            yubild.cmake("   ")

    def test_post_build_copy_defaults_to_exe_dir(self) -> None:
        yubild.project(name="demo")
        yubild.executable(
            "app", sources=["a.cpp"], post_build=[yubild.copy("libcef.so")]
        )
        step = _state.export()["targets"][0]["post_build"][0]
        self.assertEqual(step["kind"], "copy")
        self.assertEqual(step["sources"], ["libcef.so"])
        self.assertEqual(step["dest"], "@exe_dir")

    def test_post_build_copy_dir_and_explicit_dest(self) -> None:
        yubild.project(name="demo")
        yubild.executable(
            "app",
            sources=["a.cpp"],
            post_build=[
                yubild.copy_dir("resources", dest="exe_dir"),
                yubild.copy("a.dat", "b.dat", dest="@build_dir/staging"),
                yubild.cmd("${CMAKE_COMMAND}", "-E", "touch", "@exe_dir/marker"),
            ],
        )
        steps = _state.export()["targets"][0]["post_build"]
        self.assertEqual(steps[0], {"kind": "copy_dir", "src": "resources", "dest": "@exe_dir"})
        self.assertEqual(steps[1]["sources"], ["a.dat", "b.dat"])
        self.assertEqual(steps[1]["dest"], "@build_dir/staging")
        self.assertEqual(steps[2]["kind"], "command")
        self.assertEqual(steps[2]["args"][-1], "@exe_dir/marker")

    def test_post_build_rejects_bare_string(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.executable("app", sources=["a.cpp"], post_build=["cp a b"])

    def test_post_build_absent_by_default(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        self.assertEqual(_state.export()["targets"][0]["post_build"], [])


    def test_test_target_is_executable_and_flagged(self) -> None:
        yubild.project(name="demo")
        yubild.static_library("lib", sources=["b.cpp"])
        yubild.test("suite", sources=["a.cpp"], deps=["lib"])
        targets = _state.export()["targets"]
        t = next(x for x in targets if x["name"] == "suite")
        self.assertEqual(t["type"], "executable")
        self.assertTrue(t["test"])

    def test_regular_target_is_not_test(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        self.assertFalse(_state.export()["targets"][0].get("test", False))

    def test_test_rejects_post_build(self) -> None:
        yubild.project(name="demo")
        with self.assertRaises(yubild.YubildError):
            yubild.test("suite", sources=["a.cpp"], post_build=[yubild.copy("x")])


    def test_symlink_default_dest(self) -> None:
        yubild.project(name="demo")
        yubild.executable(
            "app", sources=["a.cpp"], post_build=[yubild.symlink("webui")]
        )
        step = _state.export()["targets"][0]["post_build"][0]
        self.assertEqual(step["kind"], "symlink")
        self.assertEqual(step["src"], "webui")
        self.assertEqual(step["dest"], "@exe_dir")
        self.assertIsNone(step["name"])

    def test_symlink_named(self) -> None:
        yubild.project(name="demo")
        yubild.executable(
            "app",
            sources=["a.cpp"],
            post_build=[yubild.symlink("webui", dest="build_dir", name="ui")],
        )
        step = _state.export()["targets"][0]["post_build"][0]
        self.assertEqual(step["name"], "ui")
        self.assertEqual(step["dest"], "@build_dir")

    def test_mutate_exclude_exported(self) -> None:
        yubild.project(name="demo")
        yubild.executable("app", sources=["a.cpp"])
        yubild.mutate_exclude("vendor/**", " third_party/** ")
        self.assertEqual(
            _state.export()["mutate"]["exclude"], ["vendor/**", "third_party/**"]
        )

    def test_mutate_exclude_rejects_empty(self) -> None:
        with self.assertRaises(yubild.YubildError):
            yubild.mutate_exclude("  ")


if __name__ == "__main__":
    unittest.main()
