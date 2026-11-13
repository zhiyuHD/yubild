<p align="center">
  <img src="https://icons.zhiyuhub.top/i/rocket_launch?color=6750a4&type=rounded&shape=pill&text=Yubild&size=48" alt="Yubild" />
</p>

<h1 align="center">Yubild</h1>

<p align="center"><strong>用 Python 写配置的 C++ 构建系统。</strong></p>

<p align="center">构建脚本也是代码，不该比业务代码更难写。</p>

<p align="center">
  <img src="https://icons.zhiyuhub.top/i/terminal?color=0b57d0&type=rounded&shape=pill&text=Python%20%E9%85%8D%E7%BD%AE" height="32" alt="Python 配置" />
  <img src="https://icons.zhiyuhub.top/i/devices?color=146c2e&type=rounded&shape=pill&text=Linux%20%2F%20Windows" height="32" alt="Linux 与 Windows" />
  <img src="https://icons.zhiyuhub.top/i/science?color=8f4c00&type=rounded&shape=pill&text=%E5%86%85%E7%BD%AE%E6%B5%8B%E8%AF%95" height="32" alt="内置测试" />
  <img src="https://icons.zhiyuhub.top/i/bug_report?color=b3261e&type=rounded&shape=pill&text=%E5%8F%98%E5%BC%82%E6%B5%8B%E8%AF%95" height="32" alt="变异测试" />
  <a href="https://crates.io/crates/yubild"><img src="https://icons.zhiyuhub.top/i/download?color=6750a4&type=rounded&shape=pill&text=cargo%20install%20yubild" height="32" alt="cargo install yubild" /></a>
</p>

## 这是什么

Yubild 是一个 C++ 构建系统。在项目根目录写一个 `yubild.py`——就是普通 Python——说明要编哪些库、哪些程序、依赖什么，然后执行 `yubild build`。

它会把这些声明翻译成一份 CMake 工程，再交给 cmake 和 ninja 去干活。所以你不必再学一套构建语法，跨平台和增量构建也不用重新发明一遍。

## 它能干什么

| 能力 | 它做什么 |
| :--- | --- |
| <img src="https://icons.zhiyuhub.top/i/folder?color=0b57d0&type=rounded&shape=pill&text=%E4%BE%9D%E8%B5%96%E7%AE%A1%E7%90%86" height="26" alt="依赖管理" /> | 找系统里已有的库，也能把第三方源码直接编进来 |
| <img src="https://icons.zhiyuhub.top/i/layers?color=6750a4&type=rounded&shape=pill&text=%E5%BA%93%E4%B8%8E%E7%A8%8B%E5%BA%8F" height="26" alt="库与程序" /> | 一份配置里声明库和程序，头文件目录自动传给用到它的地方 |
| <img src="https://icons.zhiyuhub.top/i/bolt?color=8f4c00&type=rounded&shape=pill&text=%E4%BA%A4%E5%8F%89%E7%BC%96%E8%AF%91" height="26" alt="交叉编译" /> | 换一个工具链，就能在 Linux 上产出 Windows 程序 |
| <img src="https://icons.zhiyuhub.top/i/download?color=146c2e&type=rounded&shape=pill&text=%E8%B5%84%E6%BA%90%E8%90%BD%E4%BD%8D" height="26" alt="资源落位" /> | 构建完自动把动态库、纹理、页面资源摆到程序旁边；开发时也可以改成软链 |
| <img src="https://icons.zhiyuhub.top/i/science?color=8f4c00&type=rounded&shape=pill&text=%E6%B5%8B%E8%AF%95" height="26" alt="测试" /> | C 和 C++ 都能写测试，挂了直接告诉你是哪个文件第几行、值是多少 |
| <img src="https://icons.zhiyuhub.top/i/bug_report?color=b3261e&type=rounded&shape=pill&text=%E5%8F%98%E5%BC%82%E6%B5%8B%E8%AF%95" height="26" alt="变异测试" /> | 自动改坏你的代码，看测试能不能发现——找出「没测到的地方」 |

## 三步开始

已经装了 Rust 的话，直接从 crates.io 装：

```bash
cargo install yubild
mkdir myapp && cd myapp
yubild init && yubild build && ./build/debug/bin/hello
```

想从源码构建，把第一行换成：

```bash
cargo install --path .        # 或 cargo build --release 后把二进制放进 PATH
```

Arch 用户可以在 AUR 里装：

```bash
yay -S yubild                 # 或 paru -S yubild
```

运行时需要 `python3` 和 `cmake`（配置层用 Python 求值，编译交给 CMake）。
`yubild init` 会生成一个 `yubild.py` 和一个 `main.cpp`，随后 `build` 就能跑通。

## 一次配置长什么样

```python
import yubild

yubild.project(name="hello", cxx_std=20)
yubild.executable("hello", sources=["src/*.cpp"])
```

想再加个库、再配一组测试，多几行就够了：

```python
yubild.static_library("core", sources=["core/*.cpp"], public_includes=["core/include"])
yubild.test("core_tests", sources=["tests/*.cpp"], deps=["core"])
```

## 想继续往下看

- [使用手册](docs/USAGE.md)：全部配置项、命令行、构建产物、测试细节
- [设计笔记](DESIGN.md)：为什么这样设计，以及踩过哪些坑
- [示例工程](examples/)：hello、testing、cef

## 现在能用吗

能用。本机构建、交叉编译、依赖管理、测试与变异测试都在实际工程上跑过。

还没验证的：Windows/MSVC 本机路径（交叉编译到 Windows 是通过的），以及 `git` 依赖只做到了生成、没跑通真实拉取。

## 许可

[MIT](LICENSE)
