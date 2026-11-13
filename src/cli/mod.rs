//! yubild 命令行入口。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::model::{Config, Target, TargetKind};

pub(crate) mod mutator;
pub(crate) mod precheck;
pub(crate) mod python;
pub(crate) mod testrunner;
pub(crate) mod util;

use testrunner::Painter;

const HELP: &str = "\
yubild - C++ 构建系统（Python 配置 + CMake 后端）

用法:
  yubild build      [选项]   生成 CMakeLists.txt 并构建
  yubild test       [选项]   构建并运行测试
  yubild mutate     [选项]   变异测试：改动源码，看测试能否发现
  yubild generate   [选项]   只生成，不构建
  yubild config     [选项]   打印解析后的项目模型（JSON）
  yubild deps       [选项]   列出 target 与外部依赖
  yubild clean      [选项]   删除构建目录
  yubild init                在当前目录创建最小示例工程

选项:
  -c, --config <FILE>    指定配置文件（缺省从当前目录向上查找 yubild.py）
      --release          以 Release 构建（缺省 Debug）
      --cross            断言当前是交叉编译（需在 yubild.py 用 toolchain() 声明）
      --target <NAME>    只构建指定 target
  -j, --jobs <N>         并行度
  -G, --generator <G>    覆盖 CMake 生成器
      --filter <TEXT>    test: 只跑名字包含 TEXT 的用例；mutate: 只变异匹配的文件
      --timeout <SECS>   单个测试进程的超时（缺省 test=60, mutate=120）
      --max <N>          变异测试最多跑 N 个变异体
      --exclude <GLOB>   变异测试复制时跳过的路径（逗号分隔，可重复）
      --no-precheck      关闭变异体的 -fsyntax-only 预检
      --list             test: 列出用例；mutate: 只列出变异体
      --progress         变异测试里显示每个变异体的实时进度（默认开启）
      --quiet            只输出失败摘要
      --no-color         禁用彩色输出
      --dry-run          只打印将要执行的命令
  -v, --verbose          打印额外诊断信息
  -h, --help             显示本帮助
";

const INIT_CONFIG: &str = "\
# yubild.py —— 用 Python 描述构建，运行 `yubild build` 即可。
import yubild

yubild.project(name=\"hello\", version=\"0.1.0\", cxx_std=20)

yubild.executable(
    \"hello\",
    sources=[\"src/main.cpp\"],
)
";

const INIT_MAIN: &str = "\
#include <iostream>

int main() {
    std::cout << \"hello from yubild\\n\";
    return 0;
}
";

/// 各类默认超时与内部目录名，集中在这里，避免散落的魔法数字。
const DEFAULT_TEST_TIMEOUT_SECS: u64 = 60;
const DEFAULT_MUTATE_TIMEOUT_SECS: u64 = 120;
/// 基线要先完整构建一次，慢得多，按单次测试超时的倍数放宽
const BASELINE_TIMEOUT_MULTIPLIER: u32 = 5;
/// 生成文件放在 <build>/yubild 下
const GEN_DIR_NAME: &str = "yubild";
const TESTKIT_DIR_NAME: &str = "testkit";

/// 测试库与运行器必须版本一致，直接编译进二进制，避免装错头文件。
const TESTKIT_HEADER: &str = include_str!("../../include/yubild_test.h");

/// 自动生成的测试入口：把实现宏定义在自己这个编译单元里。
const TEST_MAIN_BODY: &str = "\
#define YB_IMPLEMENTATION
#include \"yubild_test.h\"

int main(int argc, char** argv) {
    return yb_main(argc, argv);
}
";

/// 命令行的真正入口：返回进程退出码，由 `main.rs` 负责 exit。
///
/// 退出码约定：0 成功、1 测试失败、2 构建或环境错误（变异测试据此分类）。
pub fn main_entry() -> i32 {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("yubild: error: {e:#}");
            2
        }
    }
}

fn run() -> Result<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        print!("{HELP}");
        return Ok(0);
    };

    match first.as_str() {
        "-h" | "--help" | "help" => {
            print!("{HELP}");
            return Ok(0);
        }
        "-V" | "--version" | "version" => {
            println!("yubild {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        other if other.starts_with('-') => {
            bail!("`{other}` 是选项，不是命令。命令（build/test/mutate/...）必须放在最前面")
        }
        _ => {}
    }

    let opts = Options::parse(&args[1..])?;

    match first.as_str() {
        "build" => {
            cmd_build(&opts)?;
            Ok(0)
        }
        "test" => cmd_test(&opts),
        "mutate" => cmd_mutate(&opts),
        "generate" => {
            cmd_generate(&opts)?;
            Ok(0)
        }
        "config" => {
            cmd_config(&opts)?;
            Ok(0)
        }
        "deps" => {
            cmd_deps(&opts)?;
            Ok(0)
        }
        "clean" => {
            cmd_clean(&opts)?;
            Ok(0)
        }
        "init" => {
            cmd_init(&opts)?;
            Ok(0)
        }
        other => bail!("未知命令 `{other}`；执行 `yubild --help` 查看用法"),
    }
}

#[derive(Debug, Default)]
struct Options {
    config: Option<PathBuf>,
    release: bool,
    cross: bool,
    target: Option<String>,
    jobs: Option<usize>,
    generator: Option<String>,
    filter: Option<String>,
    exclude: Option<String>,
    no_precheck: bool,
    timeout_secs: Option<u64>,
    max_mutants: Option<usize>,
    list: bool,
    quiet: bool,
    no_color: bool,
    dry_run: bool,
    verbose: bool,
}

fn take<'a>(args: &'a [String], i: usize, name: &str) -> Result<&'a str> {
    args.get(i)
        .map(|s| s.as_str())
        .with_context(|| format!("{name} 后面需要一个值"))
}

impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut o = Options::default();
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            match a {
                "-c" | "--config" => {
                    i += 1;
                    o.config = Some(PathBuf::from(take(args, i, a)?));
                }
                "--release" => o.release = true,
                "--cross" => o.cross = true,
                "--target" => {
                    i += 1;
                    o.target = Some(take(args, i, a)?.to_string());
                }
                "-j" | "--jobs" => {
                    i += 1;
                    let v = take(args, i, a)?;
                    o.jobs = Some(
                        v.parse()
                            .with_context(|| format!("{a} 需要一个正整数，收到 `{v}`"))?,
                    );
                }
                "-G" | "--generator" => {
                    i += 1;
                    o.generator = Some(take(args, i, a)?.to_string());
                }
                "--filter" => {
                    i += 1;
                    o.filter = Some(take(args, i, a)?.to_string());
                }
                "--exclude" => {
                    i += 1;
                    o.exclude = Some(take(args, i, a)?.to_string());
                }
                "--no-precheck" => o.no_precheck = true,
                "--timeout" => {
                    i += 1;
                    let v = take(args, i, a)?;
                    o.timeout_secs = Some(
                        v.parse()
                            .with_context(|| format!("{a} 需要一个整数秒数，收到 `{v}`"))?,
                    );
                }
                "--max" => {
                    i += 1;
                    let v = take(args, i, a)?;
                    o.max_mutants = Some(
                        v.parse()
                            .with_context(|| format!("{a} 需要一个正整数，收到 `{v}`"))?,
                    );
                }
                "--list" => o.list = true,
                "--quiet" => o.quiet = true,
                "--no-color" => o.no_color = true,
                "--progress" => {}
                "--dry-run" => o.dry_run = true,
                "-v" | "--verbose" => o.verbose = true,
                other => bail!("未知参数 `{other}`；执行 `yubild --help` 查看用法"),
            }
            i += 1;
        }
        Ok(o)
    }
}

// ---------------------------------------------------------------- 命令

fn cmd_build(o: &Options) -> Result<()> {
    let (root, cfg, order) = load_project(o)?;
    let gen_dir = generate(&root, &cfg, &order, o)?;
    let bin = bin_dir(&root, &cfg, o);

    configure(&gen_dir, &bin, &cfg, o)?;
    build_with_cmake(&bin, o)?;
    link_compile_commands(&root, &bin, o);

    if o.dry_run {
        eprintln!("==> 仅打印命令，未实际执行（--dry-run）");
        return Ok(());
    }

    eprintln!();
    eprintln!(
        "==> 构建完成：{} 个 target，产出目录 {}",
        order.len(),
        bin.join("bin").display()
    );
    for &i in &order {
        let t = &cfg.targets[i];
        eprintln!("    {}{}", kind_label(t.kind), t.name);
    }
    Ok(())
}

fn cmd_test(o: &Options) -> Result<i32> {
    let (root, cfg, order) = load_project(o)?;
    let painter = Painter::new(o.no_color);

    let tests: Vec<Target> = cfg.targets.iter().filter(|t| t.test).cloned().collect();
    if tests.is_empty() {
        bail!("没有测试 target；请在 yubild.py 里用 yubild.test(...) 定义");
    }

    let gen_dir = generate(&root, &cfg, &order, o)?;
    let bin = bin_dir(&root, &cfg, o);
    configure(&gen_dir, &bin, &cfg, o)?;

    let names: Vec<String> = tests.iter().map(|t| t.name.clone()).collect();
    build_targets(&bin, &names, o)?;

    if o.dry_run {
        eprintln!("==> 仅打印命令，未实际执行（--dry-run）");
        return Ok(0);
    }

    if o.list {
        for t in &tests {
            let exe = test_binary(&bin, t)?;
            println!("{}", painter.bold(&t.name));
            for case in testrunner::list_cases(&exe, o.filter.as_deref())? {
                println!("  {case}");
            }
        }
        return Ok(0);
    }

    let timeout = Duration::from_secs(o.timeout_secs.unwrap_or(DEFAULT_TEST_TIMEOUT_SECS));
    let mut total = testrunner::SuiteResult::default();
    let mut crashed_targets = 0usize;
    let mut failed_cases: Vec<(String, String, u64)> = Vec::new();

    for t in &tests {
        let exe = test_binary(&bin, t)?;
        if !o.quiet {
            println!();
            println!("{}", painter.bold(&format!("▶ {}", t.name)));
        }
        let r = testrunner::run_suite(
            &exe,
            &root,
            o.filter.as_deref(),
            &painter,
            o.verbose,
            o.quiet,
            timeout,
        )?;
        for case in &r.cases {
            if !case.ok {
                let file = relative_to(&root, &case.file);
                failed_cases.push((case.name.clone(), file, case.line));
            }
        }
        total.total += r.total;
        total.passed += r.passed;
        total.failed += r.failed;
        total.elapsed += r.elapsed;
        // 崩溃/超时与「断言失败」是两件事，不能混成同一个计数
        if r.crashed.is_some() || r.timed_out {
            crashed_targets += 1;
        }
    }

    if o.quiet && total.failed == 0 && crashed_targets == 0 {
        return Ok(0);
    }

    if !failed_cases.is_empty() && !o.quiet {
        println!();
        println!("{}", painter.bold("失败用例:"));
        for (name, file, line) in &failed_cases {
            let at = if file.is_empty() {
                String::new()
            } else if *line > 0 {
                format!("  ({file}:{line})")
            } else {
                format!("  ({file})")
            };
            println!("  {} {name}{at}", painter.red("✗"));
        }
    }

    println!();
    let mut summary = format!(
        "{} 个用例：{} 通过, {} 失败",
        total.passed + total.failed,
        total.passed,
        total.failed
    );
    if crashed_targets > 0 {
        summary.push_str(&format!(", {crashed_targets} 个测试进程崩溃/超时"));
    }
    summary.push_str(&format!(", 用时 {:.2}s", total.elapsed.as_secs_f64()));
    if total.failed == 0 && crashed_targets == 0 {
        println!("{}", painter.green(&summary));
        Ok(0)
    } else {
        println!("{}", painter.red(&summary));
        Ok(1)
    }
}

fn cmd_mutate(o: &Options) -> Result<i32> {
    let (root, cfg, _order) = load_project(o)?;
    let painter = Painter::new(o.no_color);

    // 被测代码 = 所有非测试 target 的源文件
    let mut files: Vec<String> = Vec::new();
    for t in &cfg.targets {
        if t.test {
            continue;
        }
        for s in &t.sources {
            if !files.iter().any(|x| x == s) {
                files.push(s.clone());
            }
        }
    }
    files.sort();
    if let Some(f) = &o.filter {
        files.retain(|p| util::wildcard_match(f, p));
    }
    if files.is_empty() {
        bail!("没有可变异的目标源码（--filter 可能过滤掉了全部文件）");
    }

    let mut mutants = Vec::new();
    for rel in &files {
        let path = root.join(rel);
        let content =
            fs::read_to_string(&path).with_context(|| format!("读取 {}", path.display()))?;
        mutants.extend(mutator::scan(rel, &content));
    }
    if mutants.is_empty() {
        bail!("这些文件里没有找到可变异的位置");
    }

    if o.list {
        for m in &mutants {
            println!("{}", m.label());
        }
        println!("共 {} 个变异体", mutants.len());
        return Ok(0);
    }

    if let Some(max) = o.max_mutants {
        mutants.truncate(max);
    }

    // 排除规则：配置文件里的 mutate_exclude() + 命令行 --exclude
    let mut excludes: Vec<String> = cfg.mutate.exclude.clone();
    if let Some(list) = &o.exclude {
        for part in list.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                excludes.push(part.to_string());
            }
        }
    }

    // 在隔离副本里变异：绝不改动用户的源码树
    let build_dir_name = cfg.project.build_dir.clone();
    let ws = root.join(&build_dir_name).join("mutants");
    if ws.exists() {
        fs::remove_dir_all(&ws).with_context(|| format!("无法清理 {}", ws.display()))?;
    }
    fs::create_dir_all(&ws)?;

    let skip_names = [
        ".git",
        "target",
        "__pycache__",
        build_dir_name.as_str(),
        "compile_commands.json",
    ];
    let exclude_refs: Vec<&str> = excludes.iter().map(|s| s.as_str()).collect();

    let copied_at = std::time::Instant::now();
    let copy = util::copy_tree_filtered(&root, &ws, &skip_names, &exclude_refs)
        .with_context(|| format!("无法创建工作副本 {}", ws.display()))?;
    println!(
        "{}",
        painter.dim(&format!(
            "工作副本: {} 个文件（硬链接 {}，复制 {}），跳过 {} 项，用时 {:.2}s",
            copy.files,
            copy.linked,
            copy.copied,
            copy.skipped,
            copied_at.elapsed().as_secs_f64()
        ))
    );

    let ws_config = ws.join("yubild.py");
    if !ws_config.is_file() {
        bail!("工作副本里没有 yubild.py，无法运行变异测试");
    }

    let this_exe = std::env::current_exe().context("无法定位 yubild 自身可执行文件")?;
    let timeout = Duration::from_secs(o.timeout_secs.unwrap_or(DEFAULT_MUTATE_TIMEOUT_SECS));

    // 基线：副本必须能构建、测试全绿，否则后面的「杀死/存活」没有意义
    let baseline_timeout = timeout * BASELINE_TIMEOUT_MULTIPLIER;
    println!("{}", painter.dim("基线：构建并运行一次测试 ..."));
    let baseline =
        util::run_with_timeout(&mut test_child(&this_exe, &ws_config), baseline_timeout)?;
    match baseline {
        util::Outcome::Exited(st) if st.success() => {}
        util::Outcome::Exited(st) => bail!(
            "基线测试未通过（退出码 {:?}）；先单独运行 `yubild test` 修好测试再做变异测试",
            st.code()
        ),
        util::Outcome::TimedOut => bail!("基线构建/测试超过 {} 秒", baseline_timeout.as_secs()),
    }

    // 预检需要项目自己的编译参数：从 compile_commands.json 读
    let bin = bin_dir(&ws, &cfg, o);
    let db = if o.no_precheck {
        None
    } else {
        precheck::CompileDb::load(&bin)
    };
    if db.is_none() && !o.no_precheck {
        eprintln!("  注意: 没有找到 compile_commands.json，本次不做语法预检");
    }

    let total = mutants.len();
    println!("{}", painter.bold(&format!("变异测试：{total} 个变异体")));

    let mut killed = 0usize;
    let mut survived = 0usize;
    let mut build_failed = 0usize;
    let mut invalid = 0usize;
    let mut survivors: Vec<&mutator::Mutant> = Vec::new();

    for (i, m) in mutants.iter().enumerate() {
        let ws_file = ws.join(&m.file);
        let original = fs::read_to_string(&ws_file)
            .with_context(|| format!("读取副本 {}", ws_file.display()))?;

        if m.end > original.len() || m.start >= m.end {
            build_failed += 1;
            continue;
        }
        let mutated = format!("{}{}{}", &original[..m.start], m.to, &original[m.end..]);
        util::write_replacing(&ws_file, &mutated)
            .with_context(|| format!("写入变异体 {}", ws_file.display()))?;

        if !o.quiet {
            print!(
                "  {} {} ... ",
                painter.dim(&format!("[{}/{}]", i + 1, total)),
                m.label()
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }

        // 预检：编译不过的变异体（文本变异必然产生一些）直接跳过，省掉一次完整重建
        if let Some(db) = &db {
            if db.syntax_check(&ws_file) == Some(false) {
                util::write_replacing(&ws_file, &original)?;
                invalid += 1;
                if !o.quiet {
                    println!("{}", painter.dim("预检跳过（编译不过）"));
                }
                continue;
            }
        }

        let outcome = util::run_with_timeout(&mut test_child(&this_exe, &ws_config), timeout);

        // 立刻复原，保证下一个变异体基于原始代码
        util::write_replacing(&ws_file, &original)
            .with_context(|| format!("复原 {}", ws_file.display()))?;

        match outcome {
            Ok(util::Outcome::TimedOut) => {
                killed += 1;
                if !o.quiet {
                    println!("{}", painter.green("杀死（超时）"));
                }
            }
            Ok(util::Outcome::Exited(status)) => match status.code() {
                Some(0) => {
                    survived += 1;
                    survivors.push(m);
                    if !o.quiet {
                        println!("{}", painter.red("存活"));
                    }
                }
                Some(1) => {
                    killed += 1;
                    if !o.quiet {
                        println!("{}", painter.green("杀死"));
                    }
                }
                _ => {
                    build_failed += 1;
                    if !o.quiet {
                        println!("{}", painter.yellow("构建失败（变异体本身不合法）"));
                    }
                }
            },
            Err(e) => {
                build_failed += 1;
                if !o.quiet {
                    println!("{}", painter.yellow(&format!("构建失败: {e}")));
                }
            }
        }
    }

    let scored = killed + survived;
    let score = if scored == 0 {
        0.0
    } else {
        killed as f64 * 100.0 / scored as f64
    };

    println!();
    println!("{}", painter.bold("变异测试结果"));
    println!(
        "  变异体 {total}   杀死 {killed}   存活 {survived}   构建失败 {build_failed}   预检跳过 {invalid}"
    );
    println!("  变异分数 {score:.1}%（分母不含构建失败与预检跳过）");
    if !survivors.is_empty() {
        println!("  存活的变异体（说明测试没有覆盖到这些行为）：");
        for m in &survivors {
            println!("    {} {}", painter.red("✗"), m.label());
        }
    }
    println!("  隔离副本：{}", ws.display());

    Ok(if survivors.is_empty() { 0 } else { 1 })
}

/// 在副本里跑测试的子进程命令（安静模式，结果只看退出码）。
fn test_child(exe: &Path, config: &Path) -> Command {
    let mut c = Command::new(exe);
    c.arg("test")
        .arg("--config")
        .arg(config)
        .arg("--no-color")
        .arg("--quiet");
    c
}

fn cmd_generate(o: &Options) -> Result<()> {
    let (root, cfg, order) = load_project(o)?;
    let gen_dir = generate(&root, &cfg, &order, o)?;
    eprintln!(
        "==> {} 个 target 已写入 {}",
        order.len(),
        gen_dir.join("CMakeLists.txt").display()
    );
    Ok(())
}

fn cmd_config(o: &Options) -> Result<()> {
    let (_, cfg, _) = load_project(o)?;
    println!("{}", serde_json::to_string_pretty(&cfg)?);
    Ok(())
}

fn cmd_deps(o: &Options) -> Result<()> {
    let (_, cfg, _) = load_project(o)?;

    println!("项目: {} {}", cfg.project.name, cfg.project.version);
    println!("C++ 标准: {}", cfg.project.cxx_std);
    if cfg.toolchain.is_cross() {
        println!(
            "交叉编译: 系统={} 编译器={}",
            cfg.toolchain.system_name.as_deref().unwrap_or("(宿主)"),
            cfg.toolchain
                .resolved_cxx()
                .unwrap_or_else(|| "(宿主默认)".into())
        );
    }

    println!("\ntarget:");
    for t in &cfg.targets {
        let deps = if t.deps.is_empty() {
            String::new()
        } else {
            format!("  <- {}", t.deps.join(", "))
        };
        let post = if t.post_build.is_empty() {
            String::new()
        } else {
            format!("  [post_build x{}]", t.post_build.len())
        };
        let test = if t.test { "  [test]" } else { "" };
        println!(
            "  {:<24} {:<16} {} 个源文件{test}{post}{deps}",
            t.name,
            t.kind.to_string(),
            t.sources.len()
        );
    }

    if !cfg.cmake.before_project.is_empty() || !cfg.cmake.after_project.is_empty() {
        println!(
            "\nCMake 片段: project() 前 {} 条, 后 {} 条",
            cfg.cmake.before_project.len(),
            cfg.cmake.after_project.len()
        );
    }

    println!("\n外部依赖:");
    if cfg.dependencies.is_empty() {
        println!("  （无，用 yubild.dependency(...) 声明）");
    }
    for d in &cfg.dependencies {
        println!(
            "  {:<24} {:<14} {}{}",
            d.name,
            d.kind.as_str(),
            d.package_name(),
            if d.required { "" } else { " (可选)" }
        );
    }
    Ok(())
}

fn cmd_clean(o: &Options) -> Result<()> {
    let (root, cfg, _) = load_project(o)?;
    let dir = root.join(&cfg.project.build_dir);
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("无法删除 {}", dir.display()))?;
        println!("已删除 {}", dir.display());
    } else {
        println!("构建目录不存在，无需清理");
    }
    Ok(())
}

fn cmd_init(_o: &Options) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let config = cwd.join("yubild.py");
    if config.exists() {
        bail!("{} 已存在，拒绝覆盖", config.display());
    }
    fs::create_dir_all(cwd.join("src"))?;
    fs::write(&config, INIT_CONFIG).with_context(|| format!("无法写入 {}", config.display()))?;
    let main_cpp = cwd.join("src").join("main.cpp");
    if !main_cpp.exists() {
        fs::write(&main_cpp, INIT_MAIN)
            .with_context(|| format!("无法写入 {}", main_cpp.display()))?;
    }
    println!("已创建 {} 和 src/main.cpp", config.display());
    println!("下一步: yubild build");
    Ok(())
}

// ---------------------------------------------------------------- 内部

/// 把 `__FILE__` 里的绝对路径缩成项目内相对路径，报错更好读。
fn relative_to(root: &Path, file: &str) -> String {
    if file.is_empty() {
        return String::new();
    }
    let p = Path::new(file);
    match p.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => file.to_string(),
    }
}

fn kind_label(k: TargetKind) -> &'static str {
    match k {
        TargetKind::Executable => "可执行文件 ",
        TargetKind::StaticLibrary => "静态库     ",
        TargetKind::SharedLibrary => "动态库     ",
    }
}

fn load_project(o: &Options) -> Result<(PathBuf, Config, Vec<usize>)> {
    let config = resolve_config(o)?;
    let root = config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let py = python::interpreter()?;
    let home = python::home()?;
    if o.verbose {
        eprintln!("==> python  : {py}");
        eprintln!("==> 包目录  : {}", home.display());
        eprintln!("==> 配置    : {}", config.display());
    }

    let mut cfg = python::run(&py, &home, &config)?;
    if cfg.source_root.is_none() {
        cfg.source_root = Some(root.to_string_lossy().to_string());
    }
    if cfg.config_file.is_none() {
        cfg.config_file = Some(config.to_string_lossy().to_string());
    }
    let order = crate::resolve_targets(&cfg)?;

    if o.cross && !cfg.toolchain.is_cross() {
        bail!("--cross 需要 yubild.py 里用 yubild.toolchain(...) 声明目标工具链");
    }

    Ok((root, cfg, order))
}

fn resolve_config(o: &Options) -> Result<PathBuf> {
    if let Some(p) = &o.config {
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            std::env::current_dir()?.join(p)
        };
        return abs
            .canonicalize()
            .with_context(|| format!("配置文件不存在: {}", abs.display()));
    }

    let start = std::env::current_dir()?;
    let mut dir = start.clone();
    loop {
        let candidate = dir.join("yubild.py");
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => bail!(
                "在 {} 及其上级目录都找不到 yubild.py；请在项目根目录执行，或用 --config 指定",
                start.display()
            ),
        }
    }
}

fn generate(root: &Path, cfg: &Config, order: &[usize], o: &Options) -> Result<PathBuf> {
    let gen_dir = root.join(&cfg.project.build_dir).join(GEN_DIR_NAME);
    fs::create_dir_all(&gen_dir).with_context(|| format!("无法创建目录 {}", gen_dir.display()))?;

    // 测试库随二进制一起分发：写进 build 目录，避免版本不一致
    let testkit_dir = gen_dir.join(TESTKIT_DIR_NAME);
    fs::create_dir_all(&testkit_dir)?;
    write_if_changed(&testkit_dir.join("yubild_test.h"), TESTKIT_HEADER)?;

    let mut test_mains: HashMap<String, String> = HashMap::new();
    for t in &cfg.targets {
        if !t.test {
            continue;
        }
        let is_c = t.sources.iter().any(|s| s.ends_with(".c"));
        let ext = if is_c { "c" } else { "cpp" };
        let path = gen_dir.join(format!("test_main_{}.{}", t.name, ext));
        write_if_changed(&path, TEST_MAIN_BODY)?;
        test_mains.insert(t.name.clone(), path.to_string_lossy().to_string());
    }

    let testkit_path = testkit_dir.to_string_lossy().to_string();
    let emit_opts = crate::cmake::EmitOptions {
        testkit_dir: Some(testkit_path.as_str()),
        test_main: Some(&test_mains),
    };
    let text = crate::cmake::emit(cfg, order, &emit_opts);

    let path = gen_dir.join("CMakeLists.txt");
    if write_if_changed(&path, &text)? {
        eprintln!("==> 已生成 {}", path.display());
    } else if o.verbose {
        eprintln!("==> {} 无变化", path.display());
    }

    let resolved = serde_json::to_string_pretty(cfg)?;
    write_if_changed(&gen_dir.join("resolved.json"), &resolved)?;

    Ok(gen_dir)
}

fn write_if_changed(path: &Path, content: &str) -> Result<bool> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(false);
        }
    }
    fs::write(path, content).with_context(|| format!("无法写入 {}", path.display()))?;
    Ok(true)
}

fn bin_dir(root: &Path, cfg: &Config, o: &Options) -> PathBuf {
    let base = root.join(&cfg.project.build_dir);
    let profile = if o.release { "release" } else { "debug" };
    if cfg.toolchain.is_cross() {
        let sys = cfg
            .toolchain
            .system_name
            .as_deref()
            .unwrap_or("cross")
            .to_ascii_lowercase();
        base.join(format!("{profile}-{sys}"))
    } else {
        base.join(profile)
    }
}

fn test_binary(bin: &Path, t: &Target) -> Result<PathBuf> {
    let name = t.output_name.clone().unwrap_or_else(|| t.name.clone());
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name
    };
    let path = bin.join("bin").join(file);
    if !path.is_file() {
        bail!(
            "找不到测试可执行文件 {}；构建可能失败了（加 -v 查看详情）",
            path.display()
        );
    }
    Ok(path)
}

/// 定位 cmake：`YUBILD_CMAKE` > `CMAKE` > PATH 里的 `cmake`。
///
/// 写死 "cmake" 在 Windows 上经常找不到（CMake 装在 Program Files 里，
/// 由 GUI 安装器写进注册表而不改 PATH），所以留出覆盖入口。
fn cmake_command() -> Command {
    let exe = std::env::var("YUBILD_CMAKE")
        .or_else(|_| std::env::var("CMAKE"))
        .unwrap_or_else(|_| "cmake".to_string());
    Command::new(exe)
}

fn configure(gen: &Path, bin: &Path, cfg: &Config, o: &Options) -> Result<()> {
    let mut c = cmake_command();
    c.arg("-S").arg(gen).arg("-B").arg(bin);
    c.arg(format!(
        "-DCMAKE_BUILD_TYPE={}",
        if o.release { "Release" } else { "Debug" }
    ));
    if let Some(g) = o
        .generator
        .clone()
        .or_else(|| cfg.toolchain.generator.clone())
    {
        c.arg("-G").arg(g);
    }
    run_command(&mut c, o)
}

fn build_with_cmake(bin: &Path, o: &Options) -> Result<()> {
    let mut c = cmake_command();
    c.arg("--build").arg(bin);
    if let Some(t) = &o.target {
        c.arg("--target").arg(t);
    }
    if let Some(j) = o.jobs {
        c.arg("--parallel").arg(j.to_string());
    }
    run_command(&mut c, o)
}

fn build_targets(bin: &Path, targets: &[String], o: &Options) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let mut c = cmake_command();
    c.arg("--build").arg(bin).arg("--target");
    for t in targets {
        c.arg(t);
    }
    if let Some(j) = o.jobs {
        c.arg("--parallel").arg(j.to_string());
    }
    run_command(&mut c, o)
}

fn run_command(cmd: &mut Command, o: &Options) -> Result<()> {
    let shown = format_command(cmd);
    eprintln!("==> {shown}");
    if o.dry_run {
        return Ok(());
    }
    let status = cmd
        .status()
        .with_context(|| format!("无法执行命令：{shown}"))?;
    if !status.success() {
        let how = status
            .code()
            .map(|c| format!("退出码 {c}"))
            .unwrap_or_else(|| "被信号终止".to_string());
        bail!("命令失败（{how}）：{shown}");
    }
    Ok(())
}

fn format_command(cmd: &Command) -> String {
    let mut parts = vec![shell_quote(&cmd.get_program().to_string_lossy())];
    for a in cmd.get_args() {
        parts.push(shell_quote(&a.to_string_lossy()));
    }
    parts.join(" ")
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || "\"'$".contains(c)) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn link_compile_commands(root: &Path, bin: &Path, o: &Options) {
    let src = bin.join("compile_commands.json");
    if !src.is_file() {
        return;
    }
    let dst = root.join("compile_commands.json");
    let _ = fs::remove_file(&dst);

    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&src, &dst);
    #[cfg(not(unix))]
    let result = fs::copy(&src, &dst).map(|_| ());

    match result {
        Ok(()) => {
            if o.verbose {
                eprintln!("==> compile_commands.json -> {}", src.display());
            }
        }
        Err(e) => eprintln!("  警告: 无法创建 compile_commands.json 链接: {e}"),
    }
}
