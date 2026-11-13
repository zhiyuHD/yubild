"""CEF 应用的 Yubild 配置示例。

准备：从 CEF Builds 下载 Standard Distribution，解压到本目录下的 `cef/`。

    cef/
    ├── cmake/          # FindCEF.cmake 等
    ├── include/
    ├── Release/        # libcef.so、chrome-sandbox、locales/、*.pak ...
    └── Resources/

这里演示 M0 新加的两个能力：
1. ``cmake()`` 注入前导片段，设置 CEF_ROOT / CMAKE_MODULE_PATH 并 find_package(CEF)；
2. ``post_build`` 把 CEF 的运行时资源摆到可执行文件旁边，否则程序启动就崩。

注意：本示例只演示 API 形状，尚未在真实 CEF 二进制上端到端验证；
不同 CEF 版本的链接目标名与资源路径不同，请按下载到的版本调整。
"""

import os

import yubild

CEF_ROOT = os.path.abspath("cef")

if not os.path.isdir(CEF_ROOT):
    raise yubild.YubildError(
        f"没有找到 CEF 二进制：{CEF_ROOT}\n"
        "请下载 CEF Standard Distribution 并解压为 ./cef/ 后再执行 yubild"
    )

yubild.project(name="cefclient", version="0.1.0", cxx_std=17)

# CEF 的 CMake 模块需要先知道 CEF_ROOT，且 find_package 必须在 project() 之后执行
yubild.cmake(f'set(CEF_ROOT "{CEF_ROOT}")')
yubild.cmake('list(APPEND CMAKE_MODULE_PATH "${CEF_ROOT}/cmake")')
yubild.cmake("find_package(CEF REQUIRED)")

yubild.executable(
    "cefclient",
    sources=["src/*.cpp"],
    include_dirs=[os.path.join(CEF_ROOT, "include")],
    link_libraries=["libcef_dll_wrapper"],  # 按你的 CEF 版本调整
    post_build=[
        # 动态库与沙箱
        yubild.copy(
            os.path.join(CEF_ROOT, "Release", "libcef.so"),
            os.path.join(CEF_ROOT, "Release", "chrome-sandbox"),
            os.path.join(CEF_ROOT, "Release", "chrome_100_percent.pak"),
            os.path.join(CEF_ROOT, "Release", "chrome_200_percent.pak"),
            os.path.join(CEF_ROOT, "Release", "icudtl.dat"),
            os.path.join(CEF_ROOT, "Release", "v8_context_snapshot.bin"),
            dest="exe_dir",
        ),
        yubild.copy_dir(os.path.join(CEF_ROOT, "Release", "locales"), dest="exe_dir"),
        yubild.copy_dir(os.path.join(CEF_ROOT, "Resources"), dest="exe_dir"),
        # 应用自己的前端资源：拷到 exe 旁边的 webui/
        yubild.copy_dir("webui", dest="exe_dir"),
    ],
)
