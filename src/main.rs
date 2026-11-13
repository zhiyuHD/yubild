//! 二进制入口。真正的逻辑在 `yubild::cli::main_entry`，这里只负责把退出码交给进程。

fn main() {
    std::process::exit(yubild::cli::main_entry());
}
