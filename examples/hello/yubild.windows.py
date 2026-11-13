import yubild

# 交叉编译到 Windows：
#   yubild --config yubild.windows.py --cross build
# 产物在 build/debug-windows/bin/hello.exe
yubild.project(name="hello", version="0.1.0", cxx_std=20)

yubild.toolchain(cross_prefix="x86_64-w64-mingw32-")

yubild.static_library(
    "greeter",
    sources=["lib/src/*.cpp"],
    public_includes=["lib/include"],
)

yubild.executable(
    "hello",
    sources=["app/main.cpp"],
    deps=["greeter"],
    # mingw 的 std::thread 依赖 winpthreads，交叉编译时需要显式 -pthread；
    # 本机 Linux 构建则由 Threads::Threads 负责（见 yubild.py）。
    compile_options=["-pthread"],
    link_options=["-pthread"],
)
