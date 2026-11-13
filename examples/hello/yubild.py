import yubild

# 最小可用的 Yubild 工程：一个静态库 + 一个可执行文件。
yubild.project(name="hello", version="0.1.0", cxx_std=20)

# Threads 是 CMake 内置模块，用来验证 find_package 这条路径真的通。
yubild.dependency("Threads", package="Threads")

yubild.static_library(
    "greeter",
    sources=["lib/src/*.cpp"],
    public_includes=["lib/include"],
)

yubild.executable(
    "hello",
    sources=["app/main.cpp"],
    deps=["greeter"],
    link_libraries=["Threads::Threads"],
)
