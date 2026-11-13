//! 变异测试：扫描源码里的可变异位置，并在隔离副本里逐个验证。

/// 一个变异体：把 [start, end) 替换成 `to`。
#[derive(Debug, Clone)]
pub struct Mutant {
    pub file: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub from: String,
    pub to: String,
}

impl Mutant {
    pub fn label(&self) -> String {
        format!("{}:{}  {} -> {}", self.file, self.line, self.from, self.to)
    }
}

/// 从源码里挑出可变异的位置。
///
/// 用轻量状态机跳过注释、字符串、字符字面量和预处理行 —— 在这些地方做文本替换
/// 只会产生无法编译或语义无关的变异体，白白拖慢一轮变异测试。
pub fn scan(file: &str, src: &str) -> Vec<Mutant> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut at_line_start = true;

    while i < b.len() {
        let c = b[i];

        // 行首（仅含空白）的 # 指令整行跳过
        if at_line_start && c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        match c {
            b'\n' => {
                line += 1;
                i += 1;
                at_line_start = true;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                i += 1;
                continue;
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    if b[i] == b'\n' {
                        line += 1;
                        at_line_start = true;
                    }
                    i += 1;
                }
                i = (i + 2).min(b.len());
                continue;
            }
            b'"' => {
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == b'"' {
                        i += 1;
                        break;
                    }
                    if b[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                continue;
            }
            b'\'' => {
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == b'\'' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            _ => {}
        }

        at_line_start = false;

        // 模板实参列表：`std::vector<char*>` 里的 </>/* 都不是可变异点
        if c == b'<' {
            if let Some(end) = template_end(b, i) {
                i = end + 1;
                continue;
            }
        }

        // 多字符运算符（最长匹配优先）
        let rest = &b[i..];
        let mut matched = false;
        for (op, repl, desc) in MULTI_OPS {
            if rest.starts_with(op.as_bytes()) {
                out.push(Mutant {
                    file: file.to_string(),
                    line,
                    start: i,
                    end: i + op.len(),
                    from: (*op).to_string(),
                    to: (*repl).to_string(),
                });
                let _ = desc;
                i += op.len();
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        // 数字字面量：只改动孤立的 0 / 1
        if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            let token = &src[start..i];
            let boundary_ok = start == 0 || !is_ident_byte(b[start - 1]);
            if boundary_ok && (token == "0" || token == "1") {
                out.push(Mutant {
                    file: file.to_string(),
                    line,
                    start,
                    end: i,
                    from: token.to_string(),
                    to: if token == "0" { "1" } else { "0" }.to_string(),
                });
            }
            continue;
        }

        // 标识符：只处理 true / false
        if c == b'_' || c.is_ascii_alphabetic() {
            let start = i;
            while i < b.len() && is_ident_byte(b[i]) {
                i += 1;
            }
            let token = &src[start..i];
            if token == "true" || token == "false" {
                out.push(Mutant {
                    file: file.to_string(),
                    line,
                    start,
                    end: i,
                    from: token.to_string(),
                    to: if token == "true" { "false" } else { "true" }.to_string(),
                });
            }
            continue;
        }

        // 单字符算术运算符：跳过一元用法（*p、-x、+1 之类）
        if let Some((_, repl)) = SINGLE_OPS.iter().find(|(ch, _)| *ch as u8 == c) {
            let declarator = c == b'*' && is_declarator_star(b, i);
            if !declarator && !is_unary_position(b, i) {
                out.push(Mutant {
                    file: file.to_string(),
                    line,
                    start: i,
                    end: i + 1,
                    from: (c as char).to_string(),
                    to: (*repl).to_string(),
                });
            }
            i += 1;
            continue;
        }

        i += 1;
    }

    out
}

const MULTI_OPS: &[(&str, &str, &str)] = &[
    ("==", "!=", "相等性取反"),
    ("!=", "==", "相等性取反"),
    ("<=", "<", "边界收紧"),
    (">=", ">", "边界收紧"),
    ("&&", "||", "逻辑与或互换"),
    ("||", "&&", "逻辑与或互换"),
    ("++", "--", "自增自减互换"),
    ("--", "++", "自增自减互换"),
];

const SINGLE_OPS: &[(char, char)] = &[
    ('<', '>'),
    ('>', '<'),
    ('+', '-'),
    ('-', '+'),
    ('*', '/'),
    ('/', '*'),
    ('%', '*'),
];

/// 模板实参列表的长度上限：超过就认为不是模板（避免把一大段表达式吞掉）
const TEMPLATE_SCAN_LIMIT: usize = 300;

const BUILTIN_TYPES: &[&str] = &[
    "int",
    "char",
    "bool",
    "float",
    "double",
    "void",
    "short",
    "long",
    "unsigned",
    "signed",
    "auto",
    "wchar_t",
    "char8_t",
    "char16_t",
    "char32_t",
    "size_t",
    "ssize_t",
    "ptrdiff_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
];

/// 判断 `<` 是否开启一个模板实参列表；是则返回匹配 `>` 的下标。
///
/// C++ 里 `<` 既可以是比较也可以是模板，纯文本无法完全区分，所以用两条保守判据：
/// 1. 内容只允许类型化 token（标识符、`::`、`,`、`*`、`&`、数字、括号、嵌套尖括号）；
/// 2. 匹配的 `>` 之后必须接上 `>`/`::`/`(`/`;`/标识符 等模板后面合法的 token。
///
/// 命中比较运算的 `&&`、`||`、`==` 会直接判为非模板。
fn template_end(b: &[u8], lt: usize) -> Option<usize> {
    // `<` 前面必须是标识符字符或 '>'（模板名之后）
    let mut j = lt;
    let mut prev_ok = false;
    while j > 0 {
        j -= 1;
        let p = b[j];
        if p == b' ' || p == b'\t' || p == b'\n' || p == b'\r' {
            continue;
        }
        prev_ok = is_ident_byte(p) || p == b'>';
        break;
    }
    if !prev_ok {
        return None;
    }

    let mut depth: usize = 0;
    let mut paren: i32 = 0;
    let mut saw_ident = false;
    let mut k = lt;
    while k < b.len() && k - lt <= TEMPLATE_SCAN_LIMIT {
        let c = b[k];
        match c {
            b'<' => {
                depth += 1;
                k += 1;
            }
            b'>' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                k += 1;
                if depth == 0 {
                    return if saw_ident && template_tail_ok(b, k) {
                        Some(k - 1)
                    } else {
                        None
                    };
                }
            }
            b'&' => {
                // `a < b && c > d` 是比较；`T&&` 的模板退化为交给语法预检
                if k + 1 < b.len() && b[k + 1] == b'&' {
                    return None;
                }
                k += 1;
            }
            b' '
            | b'\t'
            | b'\n'
            | b'\r'
            | b','
            | b'*'
            | b':'
            | b'.'
            | b'['
            | b']'
            | b'-'
            | b'0'..=b'9' => k += 1,
            b'(' => {
                paren += 1;
                k += 1;
            }
            b')' => {
                if paren == 0 {
                    return None;
                }
                paren -= 1;
                k += 1;
            }
            _ if is_ident_byte(c) => {
                saw_ident = true;
                k += 1;
            }
            _ => return None,
        }
    }
    None
}

/// `>` 之后允许出现的 token：模板后面通常接变量名、`::`、`(`、`;` 等。
fn template_tail_ok(b: &[u8], mut k: usize) -> bool {
    while k < b.len() && matches!(b[k], b' ' | b'\t' | b'\n' | b'\r') {
        k += 1;
    }
    if k >= b.len() {
        return true;
    }
    matches!(
        b[k],
        b'>' | b':' | b'(' | b')' | b',' | b';' | b'{' | b'&' | b'*' | b'[' | b'='
    ) || is_ident_byte(b[k])
}

/// 判断 `*` 是否是声明里的指针（`char* p`、`std::string* p`、`vector<int>* p`）。
///
/// 纯文本无法区分 `a * b` 与 `char* p`，这里用类型线索：
/// 前一个 token 是内置类型、以大写字母开头（PascalCase 类型）、或限定名（`std::string`），
/// 或者前一个字符是 `>`。判错的漏网变异体会被语法预检拦下。
fn is_declarator_star(b: &[u8], i: usize) -> bool {
    let mut j = i;
    while j > 0 {
        j -= 1;
        let p = b[j];
        if p == b' ' || p == b'\t' || p == b'\n' || p == b'\r' {
            continue;
        }
        if p == b'>' {
            return true;
        }
        if !is_ident_byte(p) {
            return false;
        }
        let end = j + 1;
        let mut start = j;
        while start > 0 && is_ident_byte(b[start - 1]) {
            start -= 1;
        }
        let tok = &b[start..end];
        if tok == b"operator" {
            return true; // `operator*` 是被重载的运算符名，不是乘法
        }
        if BUILTIN_TYPES.iter().any(|t| t.as_bytes() == tok) {
            return true;
        }
        if tok[0].is_ascii_uppercase() {
            return true;
        }
        // 限定名：token 前面是 `::`
        let mut k = start;
        while k > 0 && (b[k - 1] == b' ' || b[k - 1] == b'\t') {
            k -= 1;
        }
        return k >= 2 && b[k - 1] == b':' && b[k - 2] == b':';
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// 前一个非空白字符是运算符/左括号时，当前 `*`、`+`、`-` 多半是一元运算。
fn is_unary_position(b: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let mut j = i;
    while j > 0 {
        j -= 1;
        let prev = b[j];
        if prev == b' ' || prev == b'\t' || prev == b'\n' || prev == b'\r' {
            continue;
        }
        return matches!(
            prev,
            b'(' | b'['
                | b'{'
                | b','
                | b'='
                | b'<'
                | b'>'
                | b'!'
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'&'
                | b'|'
                | b'^'
                | b'~'
                | b':'
                | b';'
                | b'?'
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_comparison_flip() {
        let ms = scan("a.cpp", "int f(int a, int b) { return a == b; }");
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].from, "==");
        assert_eq!(ms[0].to, "!=");
    }

    #[test]
    fn skips_comments_and_strings() {
        let src = "\
// a == b
/* c <= d */
const char* s = \"x == y\";
char c = '=';
int r = a == b;
";
        let ms = scan("a.cpp", src);
        let cmps: Vec<&Mutant> = ms.iter().filter(|m| m.from == "==").collect();
        assert_eq!(cmps.len(), 1, "注释和字符串里的 == 不该被变异: {ms:?}");
        assert_eq!(cmps[0].line, 5);
    }

    #[test]
    fn skips_preprocessor_lines() {
        let src = "#if A == B\nint x = 1;\n#endif\n";
        let ms = scan("a.cpp", src);
        assert!(ms.iter().all(|m| m.from != "=="), "{ms:?}");
    }

    #[test]
    fn flips_literals_and_booleans() {
        let src = "int a = 0; int b = 1; bool t = true; bool f = false;";
        let ms = scan("a.c", src);
        let pairs: Vec<(String, String)> =
            ms.iter().map(|m| (m.from.clone(), m.to.clone())).collect();
        assert!(pairs.contains(&("0".into(), "1".into())));
        assert!(pairs.contains(&("1".into(), "0".into())));
        assert!(pairs.contains(&("true".into(), "false".into())));
        assert!(pairs.contains(&("false".into(), "true".into())));
    }

    #[test]
    fn skips_unary_operators() {
        let src = "int v = *p; int w = -x;";
        let ms = scan("a.cpp", src);
        assert!(ms.is_empty(), "{ms:?}");
    }

    #[test]
    fn single_char_ops_change_meaning() {
        let src = "int r = a + b; int s = c * d; int t = e % f;";
        let ms = scan("a.cpp", src);
        let pairs: Vec<(String, String)> =
            ms.iter().map(|m| (m.from.clone(), m.to.clone())).collect();
        assert!(pairs.contains(&("+".into(), "-".into())), "{pairs:?}");
        assert!(pairs.contains(&("*".into(), "/".into())), "{pairs:?}");
        assert!(pairs.contains(&("%".into(), "*".into())), "{pairs:?}");
        // 绝不能出现「变异体等于原文」的空变异
        assert!(ms.iter().all(|m| m.from != m.to), "{ms:?}");
    }

    #[test]
    fn skips_template_arguments() {
        // 真实项目里踩到的行
        let src = "std::vector<char*> BuildArgv(const std::vector<std::string>& argv) {}";
        let ms = scan("a.cpp", src);
        assert!(ms.is_empty(), "模板实参里不该产生变异体: {ms:?}");
    }

    #[test]
    fn still_mutates_comparisons() {
        let ms = scan("a.cpp", "if (a < b) { return a > b; }");
        assert!(ms.iter().any(|m| m.from == "<"), "{ms:?}");
        assert!(ms.iter().any(|m| m.from == ">"), "{ms:?}");
    }

    #[test]
    fn logical_and_is_not_a_template() {
        let ms = scan("a.cpp", "if (a < b && c > d) {}");
        assert!(
            ms.iter().any(|m| m.from == "<"),
            "逻辑表达式里的 < 应可变异: {ms:?}"
        );
    }

    #[test]
    fn skips_pointer_declarations() {
        let src = "char* p = nullptr; std::string* q = nullptr; Foo* r;";
        let ms = scan("a.cpp", src);
        assert!(ms.iter().all(|m| m.from != "*"), "{ms:?}");
    }

    #[test]
    fn keeps_multiplication() {
        let ms = scan("a.cpp", "int v = a * b;");
        assert!(ms.iter().any(|m| m.from == "*" && m.to == "/"), "{ms:?}");
    }

    #[test]
    fn records_line_numbers() {
        let src = "int a = 1;\n\n\nint b = a < 2;\n";
        let ms = scan("a.cpp", src);
        let cmp = ms.iter().find(|m| m.from == "<").expect("<= 变异");
        assert_eq!(cmp.line, 4);
    }
}
