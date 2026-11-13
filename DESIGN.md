# Yubild 设计笔记

> 记录已定方案、被否掉的方案与理由，以及尚未闭合的风险。
> 使用说明见 [docs/USAGE.md](docs/USAGE.md)，项目简介见 [README.md](README.md)。

## 1. 已定方案

| 维度 | 选择 | 定于 |
| --- | --- | --- |
| 实现语言 | Rust | 方案确认 |
| 配置格式 | Python 模块（`yubild.py`） | 方案确认 |
| 构建后端 | 生成 `CMakeLists.txt`，交给 cmake + ninja | 方案确认 |
| 目标平台 | Linux、Windows | 方案确认 |
| 首期范围 | 构建、依赖、交叉编译 | 方案确认 |

## 2. 关键决策与理由

### 2.1 生成 CMake，而不是自己调度编译器

第一版草案推荐「自己生成 build.ninja」，理由是增量正确性由 ninja 直接保证、实现量最小。
改为生成 CMake 的原因是**生态成本**：交叉编译的工具链探测、`find_package`、
IDE 与 `compile_commands.json`、MSVC 的各种特殊行为，CMake 已经全部趟过一遍。
自己写这些，工作量不是「几倍」，而是「每加一个平台翻一倍」。

代价是受 CMake 语义约束，且多了一层「生成文件」需要保证幂等。已用
`write_if_changed` + 固定输出顺序处理：同样的配置永远生成同样的字节。

### 2.2 配置用 Python，但只做「表达」

难点在于「哪些逻辑归 Python，哪些归 Rust」。结论：

- Python 侧只做**声明收集 + 即时校验**（文件是否存在、名字是否合法、glob 是否空匹配）。
- 语义解析（拓扑排序、环检测、平台推断、生成）**全部在 Rust**。

理由：一旦两边都能做排序或默认值填充，行为会随求值路径分叉，这类 bug 极难定位。

### 2.3 子进程而非内嵌解释器

pyo3 内嵌看起来更优雅（一次进程，无序列化），但在 Windows 上要处理 Python 运行时打包、
`python3XX.dll` 查找、GIL 与 panic 交互，成本远超收益。子进程方案约 30ms 启动，
序列化用 JSON，边界清晰、可独立测试。

代价：用户脚本里的 `print()` 会污染 stdout，所以 runner 在求值期间把 `stdout` 重定向到 stderr。

### 2.4 配置文件被自身遮蔽的坑

配置文件名叫 `yubild.py`，而包也叫 `yubild`。子进程若以项目根为 cwd 启动，
`import yubild` 会先命中配置文件自己，直接炸在 `AttributeError: module 'yubild' has no attribute 'project'`。

修法：子进程以「包目录」为 cwd 启动（中性目录），项目根只经由 `PYTHONPATH` 之外的、
runner 内部逻辑在**包已导入之后**加入 `sys.path`。

### 2.5 交叉构建独立目录

`build/debug` 与 `build/debug-windows` 必须分开。
共用一个目录时，第二次配置会因工具链不同而报错，或者更糟——复用上一平台的 `.obj`。

### 2.6 前导片段为什么是两个落点

CEF 这类依赖要求 `set(CEF_ROOT ...)` + `list(APPEND CMAKE_MODULE_PATH ...)` + `find_package(CEF)`，
而 `find_package` 依赖 `project()` 已探测出编译器信息；另一些设置（工具链、缓存变量）又必须在
`project()` 之前生效。只给一个「插入点」必然有一半场景是错的，所以拆成
`before_project` / `after_project` 两个阶段，各自位置固定可预测。

### 2.7 POST_BUILD：结构化原语而不是裸脚本

直接让用户写 CMake 代码最灵活，但路径语义会散落在每个人手里。做成 `copy` / `copy_dir` / `cmd`
三个原语后，边界行为可以统一钉死：

- **源相对项目根**：`cmake -E copy_if_different` 的相对路径以构建目录为基准，
  实测会直接失败（`Error copying file ... No such file or directory`）。
- **目标相对 exe 目录**：用 `$<TARGET_FILE_DIR:t>` 而非 `${CMAKE_BINARY_DIR}/bin`，
  否则 Ninja Multi-Config / MSVC 下永远拷进 Debug。
- **`copy_directory` 拷内容不拷目录名**：`copy_dir("locales")` 默认会摊平，
  这里显式补上目录名，得到 `bin/locales/`。

### 2.8 测试系统：自研单头文件库 + 行协议

没有直接接 gtest/catch2，原因是这套东西要满足三个互相牵制的约束：**C 和 C++ 都能用**、
**运行器要实时知道跑到哪了**、**失败要能定位到行**。

- 单头文件 `include/yubild_test.h`：C11 用 `_Generic` 与构造器属性注册，C++ 用静态对象注册，
  两套都实测零告警。用例注册走链接期之外的单例链表，main 由 yubild 生成（`#define YB_IMPLEMENTATION`），
  用户不需要写入口。
- 测试库随二进制用 `include_str!` 分发到 build 目录，避免「装错头文件版本」这类问题。
- 进程间协议是 `@@YUBILD@@ {json}` 行：比解析自由文本可靠，比 TAP 有结构（能带 file/line/detail）。
  用户自己的 `printf` 是非协议行，挂在当前用例上，失败时回显。
- 退出码约定 `0/1/2`（通过 / 断言失败 / 构建或环境错误）。变异测试正是靠 1 与 2 的区别分类。

颜色遵循通行约定：TTY + 未设 `NO_COLOR` 才着色。（本机恰好设了 `NO_COLOR=1`，
验证着色时要用 `env -u NO_COLOR`。）

### 2.9 变异测试：隔离副本 + 文本变异

- **文本变异而不是 AST 变异**：不引入 libclang，跨平台、零构建期依赖。代价是会产生不合法变异体，
  这类记为「构建失败」，不计入分数分母，避免虚高。
- **扫描状态机**跳过注释、字符串、字符字面量与预处理行；`*`/`+`/`-` 的一元用法也跳过。
  这些位置变异只会白耗一轮构建。
- **隔离副本**：把项目复制到 `build/mutants/` 再改，逐个复原。原地改写用户源码看似省事，
  但一个 Ctrl-C 就会留下被污染的代码，这是不可接受的。
- **副本用硬链接**：真拷贝几 GB 的 vendor 会先把时间耗在准备阶段（在 CEF 项目上实测 vendor 2.1 GB）。
  硬链接只建目录项，因此改写必须用「临时文件 + rename」——replace 只换目录项，
  共享 inode 的原文件不会被写坏。
- **排除规则**：`mutate_exclude()` 与 `--exclude`，默认排除构建目录、`.git`、`target`。
- 副本有独立构建目录，每个变异体只重编受影响的编译单元，因此增量很快。
- **编译预检**：读 `compile_commands.json`，对变异后的单元跑 `-fsyntax-only`，
  语法/语义不过的直接跳过。这一步把「构建失败」从统计里基本清零，也让每轮少一次完整重建。
- **基线校验**：副本先跑一次测试，不绿就退出。基线红时所有变异体都会「存活」，结论无意义。
- 超时按「杀死」处理（变异导致死循环也算被测出来了）。

### 2.10 扫描精度：模板、声明指针、`operator*`

文本变异最容易在「看起来像运算符、其实不是」的地方误伤。一个 CEF 项目的 `process.cpp:21` 一次踩中三类：

```cpp
std::vector<char*> BuildArgv(const std::vector<std::string>& argv)
```

- `<`/`>` 是模板定界符，不是比较；
- `char*` 的 `*` 是指针声明，不是乘法；
- `operator*` 的名字也不是乘法。

对策是两条互补的路：

1. **识别**：`<` 后用保守的类型化 token 规则找配对 `>`（含 `&&` 等比较迹象则判为比较）；
   `*` 前一个 token 是内置类型、PascalCase、限定名或 `operator` 时判为声明。
2. **兜底**：识别不了的交给 `-fsyntax-only` 预检，代价仅几十毫秒，不会污染统计。

实测效果（某 CEF 项目的 `src/process.cpp`）：变异点 185 → 156，模板行不再产生任何变异体，
预检把剩余的非法变异体挡在重建之前。

## 3. 数据流

```
yubild.py
  │  python -c "from yubild._runner import main; main()" <config>
  │  cwd = 包目录，PYTHONPATH = 包目录
  ▼
JSON 模型（project / targets / dependencies / toolchain / source_root）
  │  serde 反序列化 → yubild-core::resolve（校验 + 拓扑排序）
  ▼
Vec<usize>（被依赖者在前）
  │  yubild-cmake::emit
  ▼
build/yubild/CMakeLists.txt（幂等）
  │  cmake -S build/yubild -B build/<profile>
  ▼
ninja（由 CMake 生成）
```

该 JSON 协议是唯一契约：Python 侧改结构、Rust 侧加字段都必须过它，
两侧各自的测试可以完全独立。

## 4. 尚未闭合的风险

1. **`git`（FetchContent）依赖**只验证了生成结果，未跑通真实网络拉取。
2. **Windows/MSVC 本机构建**未在真机验证；mingw 交叉路径已实测。
3. **纯头文件库**暂不支持（`sources` 不能为空），需要接口库语法。
4. **CEF 场景**只验证了 API 形状与假 CEF 目录的资源落位，未接入真实 CEF 二进制。
5. **代码生成场景**：源文件必须在配置阶段存在，暂不支持「构建时才生成源文件」。
6. **变异测试的成本**随源码规模线性增长，且没有覆盖率筛选（每个变异体都跑全部测试）；
   大型项目需要 `--max` / `--filter` 控制规模，后续可以接覆盖率数据做筛选。
7. **测试框架刻意做小**：无 fixture、参数化、用例隔离；复杂需求应直接引入 gtest/catch2。
8. **配置改动的可见性**：只比较生成文件内容，配置里的注释变更不会触发重新生成（预期行为，
   但若将来把配置内容嵌入生成文件，需要重新评估）。

## 5. 里程碑

- **M0（已完成）** 制定模型、生成器、CLI、两个平台各跑通一次构建
- **M1** 接口库（无源文件 target）、MSVC 本机验证、`git` 依赖端到端
- **M2** 安装规则（`install()`）、导出 `yubild.pc`/CMake package config
- **M3** 远程缓存与 C++20 modules 评估
