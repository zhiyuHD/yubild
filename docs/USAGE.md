# Yubild 使用手册

> 完整的使用说明：配置项、命令行、构建产物、前导片段、测试与变异测试。
> 项目简介见 [README](../README.md)，设计取舍见 [DESIGN](../DESIGN.md)。

## 状态

M0 已可用：单/多 target、静态库与动态库、`find_package` 与 `pkg-config` 依赖、Linux 本机构建、
Linux → Windows 交叉编译、`compile_commands.json`；外加一套 C/C++ 测试系统与变异测试。
详见文末「已知限制」。

## 快速开始

```bash
cargo build --release            # 构建 yubild 本身（需要 rustc + python3 + cmake）
export PATH="$PWD/target/release:$PATH"

mkdir myproj && cd myproj
yubild init                      # 生成 yubild.py 和 src/main.cpp
yubild build                     # 产出 build/debug/bin/hello
./build/debug/bin/hello
```

示例工程（静态库 + 可执行文件 + `find_package(Threads)`）：

```bash
cd examples/hello
yubild build                     # 本机
yubild deps                      # 看 target 与依赖
yubild build --config yubild.windows.py --cross   # 交叉编译到 Windows
```

## 配置 API

```python
import yubild

yubild.project(name="demo", version="0.1.0", cxx_std=20, build_dir="build")

# 静态库：public_includes 会被依赖方自动继承
yubild.static_library(
    "core",
    sources=["core/src/*.cpp"],        # 支持 glob，按字典序展开
    public_includes=["core/include"],
    include_dirs=["core/private"],     # 仅本 target 使用
    definitions=["CORE_EXPORTS"],
    compile_options=["-Wall"],
)

# 可执行文件
yubild.executable(
    "demo",
    sources=["app/main.cpp"],
    deps=["core"],                     # 指向本项目内的其它 target
    link_libraries=["spdlog::spdlog"], # 原样交给 target_link_libraries
)

# 外部依赖
yubild.dependency("Threads", package="Threads")                    # find_package
yubild.dependency("libfmt", kind="pkg_config")                     # pkg-config，链接 PkgConfig::LIBFMT
yubild.dependency("fmt", kind="git", url="https://...", tag="10.2.1")  # FetchContent

# 交叉编译
yubild.toolchain(cross_prefix="x86_64-w64-mingw32-")   # 自动推断 CMAKE_SYSTEM_NAME=Windows
```

| API | 说明 |
| --- | --- |
| `project(name, *, version, cxx_std, description, build_dir)` | 必填且只能调用一次 |
| `executable(name, *, sources, include_dirs, definitions, compile_options, link_options, link_libraries, deps, post_build, cxx_std, output_name)` | 可执行文件 |
| `static_library(...)` / `shared_library(...)` | 同上，额外支持 `public_includes` |
| `dependency(name, *, kind, package, link, required, version, url, tag, subdir)` | `kind`: `find_package` \| `pkg_config` \| `git` \| `subdir` |
| `toolchain(*, cross_prefix, system_name, generator, cxx_compiler, c_compiler, static_runtime)` | 工具链与交叉编译 |
| `test(name, *, sources, deps, ...)` | 测试可执行文件，自动注入测试库和 main |
| `sources(*patterns)` | 展开并复用源文件列表 |
| `cmake(code, *, phase="after_project")` | 注入原始 CMake 片段，`phase` 可选 `before_project` |
| `cmd(program, *args)` | POST_BUILD 执行任意命令 |
| `copy(*sources, dest="exe_dir")` | POST_BUILD 拷贝文件到目标目录 |
| `copy_dir(src, *, dest="exe_dir")` | POST_BUILD 递归拷贝目录（保留目录名） |

## 前导片段与 POST_BUILD

这两个能力是为了接管像 CEF 这种「自带 CMake 模块 + 一堆运行时资源」的依赖。

### 前导片段

`cmake()` 直接注入 CMake 代码，有两个落点，因为 CMake 的求值顺序是硬约束：

```python
# 必须在 project() 之前：工具链、缓存变量
yubild.cmake('set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -fno-omit-frame-pointer")',
             phase="before_project")

# 默认：project() 之后、target 之前。find_package 需要编译器探测结果，只能放这里
yubild.cmake('set(CEF_ROOT "${CMAKE_SOURCE_DIR}/cef")')
yubild.cmake('list(APPEND CMAKE_MODULE_PATH "${CEF_ROOT}/cmake")')
yubild.cmake("find_package(CEF REQUIRED)")
```

也接受字符串列表，按顺序逐行输出。

### POST_BUILD

`post_build` 里的每个动作都是结构化的，而不是一段脚本，这样路径语义才能被统一处理：

```python
yubild.executable(
    "cefclient",
    sources=["src/*.cpp"],
    post_build=[
        yubild.copy("cef/Release/libcef.so", "cef/Release/icudtl.dat", dest="exe_dir"),
        yubild.copy_dir("cef/Release/locales", dest="exe_dir"),
        yubild.copy_dir("cef/Resources", dest="exe_dir"),
        yubild.copy_dir("webui", dest="exe_dir"),
        yubild.cmd("${CMAKE_COMMAND}", "-E", "touch", "@exe_dir/.staged"),
    ],
)
```

路径规则（这几条是踩出来的，不是随手定的）：

| 规则 | 说明 |
| --- | --- |
| 源路径相对**项目根** | 否则 CMake 会以构建目录为基准解析，直接报「找不到文件」 |
| 目标路径相对**exe 目录** | 写 `dest="staging"` 就是 `bin/staging`；`dest="exe_dir"` 等价 `@exe_dir` |
| `copy_dir` 保留目录名 | `bin/locales/...`，而不是把内容摊进 `bin/` |
| 用 `$<TARGET_FILE_DIR:...>` | 多配置生成器下也能落到当前配置的输出目录，不会永远拷进 Debug |

`${...}` 形式的 CMake 变量原样透传，因此 `"${CEF_ROOT}/Release/libcef.so"` 这类写法可以直接用。

### 开发期用软链，发布用拷贝

`copy_dir("webui")` 是真实拷贝，改页面必须重新构建。开发时改用 `symlink`：

```python
yubild.symlink("webui", dest="exe_dir")                 # bin/webui -> <项目>/webui
yubild.symlink("webui", dest="exe_dir", name="ui")      # 自定义链接名
```

生成的是 `cmake -E create_symlink`，并会先 `rm -f` 旧链接，所以可以反复构建。
改完源码刷新即可，不必重建。Windows 上创建符号链接需要管理员权限或开启开发者模式，
发布包仍建议用 `copy`/`copy_dir`。

完整示例见 [examples/cef/yubild.py](examples/cef/yubild.py)（需要自行下载 CEF 二进制，尚未在真实 CEF 上验证）。

## 测试

配置里用 `yubild.test()` 声明，测试源文件里用 `YB_TEST` 写用例，main 由 yubild 生成：

```python
yubild.static_library("mathx", sources=["lib/src/*.cpp"], public_includes=["lib/include"])
yubild.test("mathx_tests", sources=["tests/*.cpp"], deps=["mathx"])
yubild.test("c_tests", sources=["tests_c/*.c"])        # 纯 C 同样支持
```

```c
#include "yubild_test.h"

YB_TEST(add_works) {
    YB_CHECK_EQ(mathx::add(2, 3), 5);
    YB_REQUIRE(ptr != NULL);      // 失败立即结束本用例
}
```

断言宏：`YB_CHECK` / `YB_REQUIRE` / `YB_CHECK_EQ` / `YB_REQUIRE_EQ` / `YB_FAIL`。
`YB_CHECK` 失败后继续跑完用例，`YB_REQUIRE` 立即返回；每个用例只保留第一条失败信息。
C++ 下 `YB_CHECK_EQ` 会打印两侧的值（`4 != 5`），C 下通过 `_Generic` 支持整数/浮点/字符串。

运行：

```bash
yubild test                    # 构建并运行，实时显示进度
yubild test --filter clamp     # 只跑名字含 clamp 的用例
yubild test --list             # 只列出用例
```

输出是逐用例实时的，跑多少、在跑什么、哪一行断言失败都直接可见：

```
▶ mathx_tests
共 7 个用例
  [1/7] add_works ✓ 0.004ms
  [2/7] clamp_below ✗
      位置 tests/mathx_test.cpp:14
      断言 mathx::clamp(-1, 0, 5) == 0
      实际 -1 != 0
```

颜色自动判断：非 TTY、设置了 `NO_COLOR`、或显式 `--no-color` 时降级为纯文本。

退出码：`0` 全部通过、`1` 有断言失败、`2` 构建或环境错误（变异测试靠这个区分「杀死」与「构建失败」）。

## 变异测试

`yubild mutate` 会逐个改写被测代码里的运算符与常量，重建并重跑测试，看测试能不能发现改动。

```bash
yubild mutate                 # 全部变异体
yubild mutate --list          # 只列出变异位置
yubild mutate --filter 'src/*.cpp' --max 50
```

```
变异测试：7 个变异体
  [1/7] src/mathx.cpp:6  + -> - 杀死
  ...
  [7/7] src/mathx.cpp:24  * -> / 存活

变异测试结果
  变异体 7   杀死 6   存活 1   构建失败 0
  变异分数 85.7%
```

「存活」意味着这处行为改动没有被任何测试发现，通常是漏测。变异分数 = 杀死 /（杀死 + 存活），
构建失败（变异体本身不合法）不计入分母。

实现上的几个关键点：

- **绝不碰用户源码树**：先把项目带进 `build/mutants/` 副本，只在副本里改写并复原，
  所以中途 Ctrl-C 也不会留下被污染的源码。
- **副本优先硬链接**：几 GB 的 vendor 目录真拷贝会让一轮变异测试光准备就废掉。
  硬链接只建目录项；改写用「临时文件 + rename」替换目录项，不会动到共享 inode 的原文件。
- **`vendor` 这类目录要排除**：仓库里常躺着几十上百 MB 的 SDK。

  ```python
  yubild.mutate_exclude("vendor/**", "third_party/cef/**")
  ```

  也可以命令行 `--exclude 'vendor/**'`（逗号分隔，可重复）。
- **跳过注释、字符串和预处理行**；模板实参 `std::vector<char*>` 里的 `<`/`>`/`*`、
  声明位置的指针 `char* p`、`operator*` 的名字都不是可变异点。
- **编译预检**：文本变异必然产出一些编译不过的变异体。每个变异体先用项目自己的编译参数跑
  `-fsyntax-only`（不生成目标文件、不链接，几十毫秒），不合法的直接跳过并单独计数，
  不再浪费一次完整重建。`--no-precheck` 可以关掉。
- **基线校验**：副本必须先能构建且测试全绿，否则直接报错退出——基线红的项目上，
  所有变异体都会「存活」，结果毫无意义。

统计口径：`变异分数 = 杀死 /（杀死 + 存活）`，`构建失败` 与 `预检跳过` 不计入分母。

## 命令行

```bash
yubild build     [--release] [--target NAME] [-j N] [--dry-run]
yubild test      [--filter TEXT] [--list] [--timeout SECS] [--no-color]
yubild mutate    [--filter GLOB] [--max N] [--list] [--timeout SECS]
yubild generate
yubild config                  # 打印解析后的 JSON 模型
yubild deps                    # 列出 target 与依赖
yubild clean
yubild init
公共选项: -c/--config <FILE>、--cross、-G/--generator、-v/--verbose
```

配置查找：从当前目录向上找 `yubild.py`，或用 `--config` 指定（多目标平台时可以每个平台一份配置）。

## 产物布局

```
build/
├── yubild/CMakeLists.txt   # 生成的构建描述（不要手改）
├── yubild/resolved.json    # 解析后的模型，排查用
├── debug/                  # 本机构建
│   ├── bin/  lib/
│   └── compile_commands.json   # 项目根有软链接
└── debug-windows/          # 交叉构建，与本地隔离
```

## 为什么这样分层

- **Python 只负责「表达」**：`python/yubild` 是薄薄一层，收集声明并做即时校验，
  求值结果序列化成 JSON。语义解析（校验、拓扑排序、平台推断）全在 Rust，避免两处逻辑分叉。
- **用子进程而不是内嵌解释器**：省掉 pyo3 在 Windows 上的打包地狱，代价约 30ms 启动，
  对构建系统无感。`PYTHONPATH` 注入包目录、子进程以中性目录启动，
  避免 `yubild.py` 遮蔽同名包。
- **生成 CMake 而不是生成 ninja 文件**：跨平台工具链文件、`find_package`、
  IDE 集成都由 CMake 负责；生成必须是幂等的，否则每次构建都会触发重新配置。
- **交叉构建独立目录**：两个工具链共用目标文件必然互相覆盖。

## 测试

```bash
cargo test                                  # Rust：排序、环路、生成器
python3 -m unittest discover -s tests       # Python：配置 DSL 校验
./tests/e2e.sh                              # 端到端：init→build→运行→增量→交叉
```

## 已知限制

- `git`（FetchContent）依赖只做了代码生成，尚未端到端验证。
- 交叉编译只验证了 Linux → Windows（mingw-w64）；Windows/MSVC 本机构建未在真机验证。
- 源文件必须在配置阶段就存在（暂不支持代码生成的产物）。
- 无 C++20 modules、无分布式/远程缓存；增量能力完全来自 CMake + ninja。
- target 名会直接作为 CMake target 名，因此限制为 `[A-Za-z_][A-Za-z0-9_-]*`。
- 测试框架是刻意做小的：没有 fixture、参数化、用例间共享状态；需要这些请直接引入 gtest/catch2。
- 变异测试串行执行，且只变异 target 的源文件（不含头文件、不做事先的覆盖率筛选），
  大型项目上建议配 `--max`、`--filter`、`--exclude` 使用。
- 硬链接副本依赖工作副本与项目在同一文件系统；跨设备时会自动退化为普通复制。
- `-fsyntax-only` 预检只能发现单个编译单元的错误；会影响其它文件（头文件、链接）的变异体，
  仍要靠完整重建判定。
