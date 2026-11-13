"""在子进程里求值用户的 ``yubild.py``，把结果以 JSON 写到 stdout。

约定：
* 退出码 0 = 成功，stdout 是 JSON；
* 退出码 2 = 环境问题（找不到文件等）；
* 退出码 3 = 配置错误，stderr 已有可读信息。

用户脚本里的 ``print()`` 会被临时重定向到 stderr，保证 stdout 只有 JSON。
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import traceback


def main() -> None:
    if len(sys.argv) < 2:
        print("用法: python -c 'from yubild._runner import main; main()' <config.py>", file=sys.stderr)
        sys.exit(2)

    config_path = os.path.abspath(sys.argv[1])
    if not os.path.isfile(config_path):
        print(f"配置文件不存在: {config_path}", file=sys.stderr)
        sys.exit(2)

    project_dir = os.path.dirname(config_path)
    # 相对路径（含 glob）统一以项目根为基准
    try:
        os.chdir(project_dir)
    except OSError as exc:
        print(f"无法切换到项目目录 {project_dir}: {exc}", file=sys.stderr)
        sys.exit(2)

    # 允许配置脚本 import 同目录下的辅助模块
    if project_dir not in sys.path:
        sys.path.insert(0, project_dir)

    real_stdout = sys.stdout
    sys.stdout = sys.stderr  # 防止用户 print() 污染 JSON 通道

    from yubild import YubildError

    try:
        spec = importlib.util.spec_from_file_location("yubild_config", config_path)
        if spec is None or spec.loader is None:
            print(f"无法加载配置文件: {config_path}", file=sys.stderr)
            sys.exit(2)
        module = importlib.util.module_from_spec(spec)
        sys.modules["yubild_config"] = module
        spec.loader.exec_module(module)
    except SystemExit:
        raise
    except YubildError as exc:
        # 用户笔误：只给一行可读信息，不打 traceback
        print(f"配置错误: {exc}", file=sys.stderr)
        sys.exit(3)
    except BaseException:  # noqa: BLE001 - 需要把用户脚本的任意异常完整带出去
        traceback.print_exc()
        sys.exit(3)
    finally:
        sys.stdout = real_stdout

    from yubild import _state

    try:
        payload = _state.export()
    except YubildError as exc:
        print(f"配置错误: {exc}", file=sys.stderr)
        sys.exit(3)
    except BaseException:  # noqa: BLE001
        traceback.print_exc()
        sys.exit(3)

    payload["source_root"] = project_dir.replace(os.sep, "/")
    payload["config_file"] = config_path.replace(os.sep, "/")

    json.dump(payload, real_stdout, ensure_ascii=False)
    real_stdout.write("\n")
    real_stdout.flush()


if __name__ == "__main__":
    main()
