#!/usr/bin/env bash
# Yubild 端到端测试：在临时目录里走完 init -> build -> 运行 -> 增量 -> 交叉编译。
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
YUBILD="$ROOT/target/debug/yubild"
PASS=0
FAIL=0

ok()   { PASS=$((PASS + 1)); printf '  \033[32mok\033[0m   %s\n' "$1"; }
bad()  { FAIL=$((FAIL + 1)); printf '  \033[31mFAIL\033[0m %s\n' "$1"; [ -n "${2:-}" ] && printf '       %s\n' "$2"; }
step() { printf '\n== %s\n' "$1"; }

if [ ! -x "$YUBILD" ]; then
    echo "构建 yubild ..."
    cargo build --manifest-path "$ROOT/Cargo.toml" >/dev/null || { echo "cargo build 失败"; exit 1; }
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

step "init 生成最小工程"
"$YUBILD" init >/dev/null 2>&1 && ok "yubild init" || bad "yubild init"
[ -f yubild.py ] && [ -f src/main.cpp ] && ok "生成了 yubild.py 与 src/main.cpp" || bad "缺少脚手架文件"

step "Debug 构建"
if "$YUBILD" build >"$WORK/build.log" 2>&1; then ok "yubild build"; else bad "yubild build" "$(tail -5 "$WORK/build.log")"; fi
if [ -x build/debug/bin/hello ]; then ok "产出 build/debug/bin/hello"; else bad "缺少可执行文件"; fi

step "运行产物"
out=$(./build/debug/bin/hello 2>&1 || true)
[ "$out" = "hello from yubild" ] && ok "输出正确: $out" || bad "输出不符: $out"

step "compile_commands.json"
[ -e compile_commands.json ] && ok "项目根有 compile_commands.json" || bad "缺少 compile_commands.json"

step "改配置后重新生成"
# 注意：管道 + grep -q 在 pipefail 下会因 SIGPIPE 误判，这里先取变量再匹配。
gen_out=$("$YUBILD" build -v 2>&1)
case "$gen_out" in
    *无变化*) ok "配置未变时不重写 CMakeLists.txt" ;;
    *) bad "生成不幂等" "$gen_out" ;;
esac

# 只改注释不该触发重新生成；改语义才应该触发。
echo '    # 仅注释' >> yubild.py
gen_out=$("$YUBILD" build -v 2>&1)
case "$gen_out" in
    *无变化*) ok "仅注释变更不触发重新生成" ;;
    *) bad "注释变更误触发重新生成" "$gen_out" ;;
esac

sed -i 's/cxx_std=20/cxx_std=17/' yubild.py
gen_out=$("$YUBILD" build -v 2>&1)
case "$gen_out" in
    *已生成*) ok "语义变更触发重新生成" ;;
    *) bad "语义变更未触发重新生成" "$gen_out" ;;
esac
sed -i 's/cxx_std=17/cxx_std=20/' yubild.py
"$YUBILD" build >/dev/null 2>&1

step "Release 构建"
if "$YUBILD" build --release >/dev/null 2>&1 && [ -x build/release/bin/hello ]; then
    ok "Release 产物存在"
else
    bad "Release 构建失败"
fi

step "前导片段与 POST_BUILD"
mkdir -p "$WORK/stage/src" "$WORK/stage/data/assets/sub"
printf 'int main() { return 0; }\n' > "$WORK/stage/src/main.cpp"
printf 'conf=1\n' > "$WORK/stage/data/app.conf"
printf 'asset\n' > "$WORK/stage/data/assets/sub/icon.txt"
cat > "$WORK/stage/yubild.py" <<'PYEOF'
import yubild

yubild.project(name="stage", cxx_std=20)

# 必须在 project() 之前生效的片段
yubild.cmake('set(YUBILD_E2E_MARKER 1)', phase="before_project")

# project() 之后校验：顺序错了这里会 FATAL_ERROR
yubild.cmake('if(NOT YUBILD_E2E_MARKER)')
yubild.cmake('  message(FATAL_ERROR "before_project 片段没有生效")')
yubild.cmake('endif()')

yubild.executable(
    "stage",
    sources=["src/main.cpp"],
    post_build=[
        yubild.copy("data/app.conf", dest="exe_dir"),
        yubild.copy_dir("data/assets", dest="exe_dir"),
        yubild.cmd("${CMAKE_COMMAND}", "-E", "touch", "@exe_dir/cmd_marker"),
    ],
)
PYEOF

if (cd "$WORK/stage" && "$YUBILD" build >"$WORK/stage.log" 2>&1); then
    ok "带前导片段与 POST_BUILD 的构建成功"
else
    bad "构建失败" "$(tail -6 "$WORK/stage.log")"
fi
stage_bin="$WORK/stage/build/debug/bin"
if [ -f "$stage_bin/app.conf" ]; then ok "copy 落到 exe 所在目录"; else bad "app.conf 未落到 bin/"; fi
if [ -f "$stage_bin/assets/sub/icon.txt" ]; then ok "copy_dir 递归拷贝成功"; else bad "资源目录未拷贝"; fi
if [ -f "$stage_bin/cmd_marker" ]; then ok "cmd() 在 POST_BUILD 中执行成功"; else bad "cmd() 未执行"; fi

step "CEF 风格资源落位（用假 CEF 目录树）"
mkdir -p "$WORK/cefapp/src" "$WORK/cefapp/cef/Release/locales" \
         "$WORK/cefapp/cef/Resources" "$WORK/cefapp/webui"
printf 'int main(){return 0;}\n' > "$WORK/cefapp/src/main.cpp"
: > "$WORK/cefapp/cef/Release/libcef.so"
: > "$WORK/cefapp/cef/Release/icudtl.dat"
: > "$WORK/cefapp/cef/Release/locales/zh-CN.pak"
: > "$WORK/cefapp/cef/Resources/cef.pak"
: > "$WORK/cefapp/webui/index.html"
cat > "$WORK/cefapp/yubild.py" <<'PYEOF'
import os

import yubild

CEF = os.path.abspath("cef")

yubild.project(name="cefapp", cxx_std=17)

yubild.cmake(f'set(CEF_ROOT "{CEF}")')
yubild.cmake('if(NOT EXISTS "${CEF_ROOT}")')
yubild.cmake('  message(FATAL_ERROR "CEF_ROOT 没有传到 CMake")')
yubild.cmake('endif()')

yubild.executable(
    "cefapp",
    sources=["src/main.cpp"],
    post_build=[
        yubild.copy(
            os.path.join(CEF, "Release", "libcef.so"),
            os.path.join(CEF, "Release", "icudtl.dat"),
            dest="exe_dir",
        ),
        yubild.copy_dir(os.path.join(CEF, "Release", "locales"), dest="exe_dir"),
        yubild.copy_dir(os.path.join(CEF, "Resources"), dest="exe_dir"),
        yubild.copy_dir("webui", dest="exe_dir"),
    ],
)
PYEOF

if (cd "$WORK/cefapp" && "$YUBILD" build >"$WORK/cef.log" 2>&1); then
    ok "CEF 风格配置构建成功"
else
    bad "CEF 风格构建失败" "$(tail -6 "$WORK/cef.log")"
fi
cef_bin="$WORK/cefapp/build/debug/bin"
for f in libcef.so icudtl.dat locales/zh-CN.pak Resources/cef.pak webui/index.html; do
    if [ -f "$cef_bin/$f" ]; then ok "落位正确: bin/$f"; else bad "缺少 bin/$f"; fi
done

step "测试系统"
mkdir -p "$WORK/t/lib" "$WORK/t/tests" "$WORK/t/tests_c"
cat > "$WORK/t/lib/mathx.cpp" <<'EOF'
#include "mathx.h"

int add(int a, int b) { return a + b; }
int is_even(int v) { return v % 2 == 0; }
int untested(int v) { return v * 2; }
EOF
cat > "$WORK/t/lib/mathx.h" <<'EOF'
#pragma once
int add(int a, int b);
int is_even(int v);
int untested(int v);
EOF
cat > "$WORK/t/tests/a.cpp" <<'EOF'
#include "mathx.h"
#include "yubild_test.h"

YB_TEST(add_works) { YB_CHECK_EQ(add(2, 3), 5); }
YB_TEST(even_true) { YB_CHECK(is_even(4)); }
YB_TEST(even_false) { YB_CHECK(!is_even(3)); }
EOF
cat > "$WORK/t/tests_c/c.c" <<'EOF'
#include "yubild_test.h"

YB_TEST(c_works) { YB_CHECK_EQ(2 + 2, 4); }
YB_TEST(c_str) { const char* s = "yubild"; YB_CHECK_EQ(s, "yubild"); }
EOF
cat > "$WORK/t/yubild.py" <<'EOF'
import yubild

yubild.project(name="t", cxx_std=17)
yubild.static_library("mathx", sources=["lib/mathx.cpp"], public_includes=["lib"])
yubild.test("cpp_tests", sources=["tests/a.cpp"], deps=["mathx"])
yubild.test("c_tests", sources=["tests_c/c.c"])
EOF

out=$(cd "$WORK/t" && "$YUBILD" test --no-color 2>&1); code=$?
[ "$code" -eq 0 ] && ok "测试全部通过（退出码 0）" || bad "测试应通过" "$out"
case "$out" in
    *"5 个用例"*) ok "C 与 C++ 用例都被执行（5 个）" ;;
    *) bad "用例数量不符" "$out" ;;
esac
case "$out" in
    *"✓"*) ok "通过用例有结果标记" ;;
    *) bad "缺少通过标记" ;;
esac
[ "$(printf '%s' "$out" | grep -c $'\033')" -eq 0 ] && ok "非 TTY 下自动关闭颜色" || bad "非 TTY 仍输出颜色"

out=$(cd "$WORK/t" && "$YUBILD" test --list 2>/dev/null)
case "$out" in
    *add_works*) ok "--list 列出用例名" ;;
    *) bad "--list 失败" "$out" ;;
esac

out=$(cd "$WORK/t" && "$YUBILD" test --filter even --no-color 2>&1)
case "$out" in
    *"2 个用例"*) ok "--filter 只跑匹配的用例" ;;
    *) bad "--filter 无效" "$out" ;;
esac

# 失败诊断：退出码 1，并给出行号与断言
mkdir -p "$WORK/tfail"
cat > "$WORK/tfail/t.cpp" <<'EOF'
#include "yubild_test.h"

YB_TEST(ok_case) { YB_CHECK(1 == 1); }
YB_TEST(bad_case) { YB_CHECK_EQ(2 + 2, 5); }
EOF
cat > "$WORK/tfail/yubild.py" <<'EOF'
import yubild
yubild.project(name="tfail", cxx_std=17)
yubild.test("tfail", sources=["t.cpp"])
EOF
out=$(cd "$WORK/tfail" && "$YUBILD" test --no-color 2>&1); code=$?
[ "$code" -eq 1 ] && ok "有失败时退出码为 1" || bad "退出码应为 1，实际 $code"
case "$out" in
    *"t.cpp:4"*) ok "失败定位到行号" ;;
    *) bad "失败行号缺失" "$out" ;;
esac
case "$out" in
    *"4 != 5"*) ok "失败给出实际值" ;;
    *) bad "缺少实际值" "$out" ;;
esac

step "变异测试"
out=$(cd "$WORK/t" && "$YUBILD" mutate --no-color --max 4 2>&1); code=$?
case "$out" in
    *"变异分数"*) ok "输出变异分数" ;;
    *) bad "缺少变异分数" "$out" ;;
esac
[ "$code" -eq 0 ] || [ "$code" -eq 1 ] && ok "变异测试正常结束（退出码 $code）" || bad "变异测试异常退出: $code"
case "$out" in
    *"存活"*) ok "报告了存活变异体" ;;
    *) bad "缺少存活报告" "$out" ;;
esac
grep -q "return v \* 2" "$WORK/t/lib/mathx.cpp" && ok "变异后用户源码已复原" || bad "源码被变异污染"

out=$(cd "$WORK/t" && "$YUBILD" mutate --list 2>&1)
case "$out" in
    *"变异体"*) ok "mutate --list 可用" ;;
    *) bad "mutate --list 失败" "$out" ;;
esac

step "变异测试质量：模板尖括号 / 声明指针 / 预检 / 排除"
mkdir -p "$WORK/q/src" "$WORK/q/tests" "$WORK/q/vendor/sdk"
cat > "$WORK/q/src/p.cpp" <<'EOF'
#include <string>
#include <vector>

std::vector<char*> BuildArgv(const std::vector<std::string>& argv) {
  std::vector<char*> out;
  for (const auto& a : argv) out.push_back(const_cast<char*>(a.c_str()));
  return out;
}

int add(int a, int b) { return a + b; }

int clamp(int v, int lo, int hi) {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}

struct Vec { int x; };
Vec operator*(const Vec& v, int k) { return Vec{v.x * k}; }
Vec scale(Vec v) { return v * 2; }
EOF
cat > "$WORK/q/src/p.h" <<'EOF'
#pragma once
#include <string>
#include <vector>
std::vector<char*> BuildArgv(const std::vector<std::string>& argv);
int add(int a, int b);
int clamp(int v, int lo, int hi);
struct Vec { int x; };
Vec operator*(const Vec& v, int k);
Vec scale(Vec v);
EOF
cat > "$WORK/q/tests/t.cpp" <<'EOF'
#include "p.h"
#include "yubild_test.h"

YB_TEST(add_works) { YB_CHECK_EQ(add(2, 3), 5); }
YB_TEST(clamp_below) { YB_CHECK_EQ(clamp(-1, 0, 5), 0); }
YB_TEST(clamp_above) { YB_CHECK_EQ(clamp(9, 0, 5), 5); }
YB_TEST(argv_shape) { YB_CHECK_EQ(BuildArgv({"a", "b"}).size(), 2u); }
YB_TEST(scale_works) { YB_CHECK_EQ(scale(Vec{2}).x, 4); }
EOF
dd if=/dev/zero of="$WORK/q/vendor/sdk/cef.bin" bs=1M count=8 2>/dev/null
cat > "$WORK/q/yubild.py" <<'EOF'
import yubild

yubild.project(name="q", cxx_std=17)
yubild.mutate_exclude("vendor/**")
yubild.static_library("p", sources=["src/p.cpp"], public_includes=["src"])
yubild.test("p_tests", sources=["tests/t.cpp"], deps=["p"])
EOF

list_out=$(cd "$WORK/q" && "$YUBILD" mutate --list --no-color 2>&1)
case "$list_out" in
    *"src/p.cpp:3 "*|*"src/p.cpp:4 "*)
        bad "模板尖括号/声明指针仍被当成变异点" "$list_out" ;;
    *) ok "模板实参里不再产生变异体" ;;
esac
case "$list_out" in
    *"src/p.cpp:8  * -> /"*) bad "指针声明仍被当成乘法变异" ;;
    *"src/p.cpp:13  < -> >"*) ok "真正的比较运算仍可变异" ;;
    *) bad "比较运算没有被变异" "$list_out" ;;
esac

mut_out=$(cd "$WORK/q" && "$YUBILD" mutate --no-color 2>&1)
case "$mut_out" in
    *硬链接*) ok "工作副本使用硬链接" ;;
    *) bad "未使用硬链接" "$mut_out" ;;
esac
case "$mut_out" in
    *预检跳过*) ok "非法变异体被 -fsyntax-only 预检拦下" ;;
    *) bad "预检没有生效" "$mut_out" ;;
esac
case "$mut_out" in
    *"构建失败 0"*) ok "没有变异体靠重建才发现不合法" ;;
    *) bad "仍有变异体进入重建后才失败" "$mut_out" ;;
esac
[ ! -e "$WORK/q/build/mutants/vendor" ] && ok "vendor 目录被排除，未复制进副本" || bad "vendor 仍被复制"
grep -q "return a + b" "$WORK/q/src/p.cpp" && ok "变异后源码完好" || bad "源码被污染"

step "符号链接动作"
mkdir -p "$WORK/link/src" "$WORK/link/webui"
printf 'int main(){return 0;}\n' > "$WORK/link/src/main.cpp"
printf 'v1\n' > "$WORK/link/webui/index.html"
cat > "$WORK/link/yubild.py" <<'EOF'
import yubild
yubild.project(name="link", cxx_std=17)
yubild.executable("link", sources=["src/main.cpp"], post_build=[
    yubild.symlink("webui", dest="exe_dir"),
])
EOF
if (cd "$WORK/link" && "$YUBILD" build >"$WORK/link.log" 2>&1); then
    ok "带 symlink 的构建成功"
else
    bad "symlink 构建失败" "$(tail -5 "$WORK/link.log")"
fi
[ -L "$WORK/link/build/debug/bin/webui" ] && ok "bin/webui 是符号链接" || bad "bin/webui 不是符号链接"
printf 'v2\n' > "$WORK/link/webui/index.html"
[ "$(cat "$WORK/link/build/debug/bin/webui/index.html")" = "v2" ]     && ok "改源码无需重建即可生效" || bad "符号链接未指向源码"

step "错误诊断"
mkdir -p "$WORK/bad" && cd "$WORK/bad"
printf 'import yubild\nyubild.project(name="cyc")\n' > yubild.py
touch x.cpp
cat >> yubild.py <<'EOF'
yubild.static_library("a", sources=["x.cpp"], deps=["b"])
yubild.static_library("b", sources=["x.cpp"], deps=["a"])
EOF
msg=$("$YUBILD" config 2>&1 || true)
case "$msg" in
    *循环依赖*) ok "循环依赖被检出" ;;
    *) bad "循环依赖未被检出" "$msg" ;;
esac
cd "$WORK"

step "交叉编译（需要 mingw-w64）"
if command -v x86_64-w64-mingw32-g++ >/dev/null 2>&1; then
    cat > yubild.windows.py <<'EOF'
import yubild
yubild.project(name="hello", cxx_std=20)
yubild.toolchain(cross_prefix="x86_64-w64-mingw32-")
yubild.executable("hello", sources=["src/main.cpp"])
EOF
    if "$YUBILD" build --config yubild.windows.py --cross >"$WORK/cross.log" 2>&1; then
        if [ -f build/debug-windows/bin/hello.exe ]; then
            ok "产出 build/debug-windows/bin/hello.exe"
            file build/debug-windows/bin/hello.exe 2>/dev/null | grep -q "PE32+" \
                && ok "产物是 PE32+（Windows 格式）" || bad "产物不是 PE32+"
        else
            bad "缺少 hello.exe"
        fi
    else
        bad "交叉编译失败" "$(tail -5 "$WORK/cross.log")"
    fi
else
    echo "  跳过：未安装 mingw-w64"
fi

printf '\n结果: %d 通过, %d 失败\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
