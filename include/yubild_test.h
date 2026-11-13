/*
 * yubild_test.h —— Yubild 的 C/C++ 测试库（单头文件）。
 *
 * 用法（测试源文件里）：
 *
 *     #include "yubild_test.h"
 *
 *     YB_TEST(add_works) {
 *         YB_CHECK_EQ(1 + 1, 2);
 *         YB_REQUIRE(ptr != NULL);
 *     }
 *
 * main() 由 yubild 自动生成，不需要自己写。
 *
 * 与运行器的通信协议：stdout 上以 "@@YUBILD@@ " 开头的行是 JSON 事件，
 * 其余输出原样保留（失败时由运行器回显）。
 */

#ifndef YUBILD_TEST_H
#define YUBILD_TEST_H

#include <stdio.h>
#include <string.h>
#include <time.h>

#ifdef __cplusplus
#  include <cstring>
#  include <exception>
#  include <sstream>
#  include <string>
#endif

#define YB_PROTOCOL "@@YUBILD@@ "

/* 头文件里的 static 辅助函数在单个编译单元里可能未被使用 */
#if defined(__GNUC__) || defined(__clang__)
#  define YB_UNUSED __attribute__((unused))
#else
#  define YB_UNUSED
#endif

#ifdef __cplusplus
extern "C" {
#endif

void yb_register(const char* name, void (*fn)(void));
void yb_record_fail(const char* file, int line, const char* expr, const char* detail);

#ifdef __cplusplus
}
#endif

/* ---------------------------------------------------------------- 注册 */

#define YB_CAT_(a, b) a##b
#define YB_CAT(a, b) YB_CAT_(a, b)

#if defined(__cplusplus)
#  define YB_REGISTRAR(fn)                                                   \
    namespace {                                                              \
    struct YB_CAT(yb_reg_obj_, fn) {                                         \
        YB_CAT(yb_reg_obj_, fn)() { fn(); }                                  \
    };                                                                       \
    static YB_CAT(yb_reg_obj_, fn) YB_CAT(yb_reg_inst_, fn);                 \
    }
#elif defined(_MSC_VER)
#  pragma section(".CRT$XCU", read)
#  define YB_REGISTRAR(fn)                                                   \
    static void YB_CAT(yb_reg_call_, fn)(void) { fn(); }                     \
    __declspec(allocate(".CRT$XCU"))                                         \
    static void (*YB_CAT(yb_reg_ptr_, fn))(void) = YB_CAT(yb_reg_call_, fn);
#else
#  define YB_REGISTRAR(fn)                                                   \
    static void __attribute__((constructor)) YB_CAT(yb_reg_call_, fn)(void) { fn(); }
#endif

#define YB_TEST(name)                                                        \
    static void YB_CAT(yb_case_, name)(void);                                \
    static void YB_CAT(yb_reg_, name)(void) {                                \
        yb_register(#name, YB_CAT(yb_case_, name));                          \
    }                                                                        \
    YB_REGISTRAR(YB_CAT(yb_reg_, name))                                      \
    static void YB_CAT(yb_case_, name)(void)

/* ---------------------------------------------------------------- 断言 */

/* YB_CHECK 失败后继续执行；YB_REQUIRE 失败后立刻结束当前用例。 */
#define YB_CHECK(expr)                                                       \
    do {                                                                     \
        if (!(expr)) {                                                       \
            yb_record_fail(__FILE__, __LINE__, #expr, "");                   \
        }                                                                    \
    } while (0)

#define YB_REQUIRE(expr)                                                     \
    do {                                                                     \
        if (!(expr)) {                                                       \
            yb_record_fail(__FILE__, __LINE__, #expr, "");                   \
            return;                                                          \
        }                                                                    \
    } while (0)

#define YB_FAIL(msg)                                                         \
    do {                                                                     \
        yb_record_fail(__FILE__, __LINE__, "YB_FAIL", (msg));                \
        return;                                                              \
    } while (0)

#ifdef __cplusplus

namespace yb {

template <class A, class B>
inline bool eq(const A& a, const B& b) {
    return a == b;
}
inline bool eq(const char* a, const char* b) {
    return a != NULL && b != NULL && std::strcmp(a, b) == 0;
}
inline bool eq(const std::string& a, const std::string& b) {
    return a == b;
}
inline bool eq(const std::string& a, const char* b) {
    return b != NULL && a == b;
}
inline bool eq(const char* a, const std::string& b) {
    return a != NULL && b == a;
}

template <class T>
inline std::string show(const T& v) {
    std::ostringstream os;
    os << v;
    return os.str();
}
inline std::string show(const char* v) {
    return v ? std::string("\"") + v + "\"" : std::string("(null)");
}
inline std::string show(const std::string& v) {
    return "\"" + v + "\"";
}

}  // namespace yb

#define YB_CHECK_EQ(a, b)                                                    \
    do {                                                                     \
        if (!::yb::eq((a), (b))) {                                           \
            std::string _yb_detail = ::yb::show(a) + " != " + ::yb::show(b); \
            yb_record_fail(__FILE__, __LINE__, #a " == " #b,                 \
                           _yb_detail.c_str());                              \
        }                                                                    \
    } while (0)

#define YB_REQUIRE_EQ(a, b)                                                  \
    do {                                                                     \
        if (!::yb::eq((a), (b))) {                                           \
            std::string _yb_detail = ::yb::show(a) + " != " + ::yb::show(b); \
            yb_record_fail(__FILE__, __LINE__, #a " == " #b,                 \
                           _yb_detail.c_str());                              \
            return;                                                          \
        }                                                                    \
    } while (0)

#else /* C */

static YB_UNUSED int yb_eq_str(const char* a, const char* b) {
    return a != NULL && b != NULL && strcmp(a, b) == 0;
}
static YB_UNUSED int yb_eq_i64(long long a, long long b) {
    return a == b;
}
static YB_UNUSED int yb_eq_f64(double a, double b) {
    return a == b;
}

static YB_UNUSED void yb_fmt_str(char* buf, size_t n, const char* a, const char* b) {
    snprintf(buf, n, "\"%s\" != \"%s\"", a ? a : "(null)", b ? b : "(null)");
}
static YB_UNUSED void yb_fmt_i64(char* buf, size_t n, long long a, long long b) {
    snprintf(buf, n, "%lld != %lld", a, b);
}
static YB_UNUSED void yb_fmt_f64(char* buf, size_t n, double a, double b) {
    snprintf(buf, n, "%g != %g", a, b);
}

#  if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L

#    define YB_EQ_CALL(a, b)                                                 \
        _Generic((a),                                                        \
            char*: yb_eq_str, const char*: yb_eq_str,                        \
            float: yb_eq_f64, double: yb_eq_f64, long double: yb_eq_f64,     \
            default: yb_eq_i64)((a), (b))

#    define YB_EQ_FMT(buf, n, a, b)                                          \
        _Generic((a),                                                        \
            char*: yb_fmt_str, const char*: yb_fmt_str,                      \
            float: yb_fmt_f64, double: yb_fmt_f64,                           \
            long double: yb_fmt_f64,                                         \
            default: yb_fmt_i64)((buf), (n), (a), (b))

#    define YB_CHECK_EQ(a, b)                                                \
        do {                                                                 \
            if (!YB_EQ_CALL(a, b)) {                                         \
                char _yb_buf[256];                                           \
                YB_EQ_FMT(_yb_buf, sizeof _yb_buf, a, b);                    \
                yb_record_fail(__FILE__, __LINE__, #a " == " #b, _yb_buf);   \
            }                                                                \
        } while (0)

#    define YB_REQUIRE_EQ(a, b)                                              \
        do {                                                                 \
            if (!YB_EQ_CALL(a, b)) {                                         \
                char _yb_buf[256];                                           \
                YB_EQ_FMT(_yb_buf, sizeof _yb_buf, a, b);                    \
                yb_record_fail(__FILE__, __LINE__, #a " == " #b, _yb_buf);   \
                return;                                                      \
            }                                                                \
        } while (0)

#  else /* 太老的 C 编译器：只比较，不打印细节 */

#    define YB_CHECK_EQ(a, b) YB_CHECK((a) == (b))
#    define YB_REQUIRE_EQ(a, b) YB_REQUIRE((a) == (b))

#  endif

#endif /* __cplusplus */

/* ---------------------------------------------------------------- 实现 */

#ifdef YB_IMPLEMENTATION

#include <stddef.h>
#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct yb_case {
    const char* name;
    void (*fn)(void);
    struct yb_case* next;
} yb_case;

static yb_case* yb_head = NULL;
static yb_case* yb_tail = NULL;

static int yb_case_failed = 0;
static const char* yb_fail_file = NULL;
static int yb_fail_line = 0;
static const char* yb_fail_expr = NULL;
static char yb_fail_detail[512];

void yb_register(const char* name, void (*fn)(void)) {
    yb_case* c = (yb_case*)malloc(sizeof(yb_case));
    if (c == NULL) {
        fprintf(stderr, "yubild_test: 内存不足，无法注册用例 %s\n", name);
        exit(2);
    }
    c->name = name;
    c->fn = fn;
    c->next = NULL;
    if (yb_tail != NULL) {
        yb_tail->next = c;
    } else {
        yb_head = c;
    }
    yb_tail = c;
}

void yb_record_fail(const char* file, int line, const char* expr, const char* detail) {
    yb_case_failed = 1;
    if (yb_fail_expr == NULL) { /* 只记录第一条失败 */
        yb_fail_file = file;
        yb_fail_line = line;
        yb_fail_expr = expr;
        snprintf(yb_fail_detail, sizeof(yb_fail_detail), "%s", detail != NULL ? detail : "");
    }
}

static YB_UNUSED void yb_put_json(const char* s) {
    putchar('"');
    if (s != NULL) {
        const unsigned char* p = (const unsigned char*)s;
        for (; *p != 0; ++p) {
            switch (*p) {
                case '"': fputs("\\\"", stdout); break;
                case '\\': fputs("\\\\", stdout); break;
                case '\n': fputs("\\n", stdout); break;
                case '\r': fputs("\\r", stdout); break;
                case '\t': fputs("\\t", stdout); break;
                default:
                    if (*p < 0x20) {
                        printf("\\u%04x", (unsigned)*p);
                    } else {
                        putchar((char)*p);
                    }
            }
        }
    }
    putchar('"');
}

static YB_UNUSED int yb_match(const char* name, const char* filter) {
    return filter == NULL || strstr(name, filter) != NULL;
}

static YB_UNUSED int yb_main(int argc, char** argv) {
    const char* filter = NULL;
    int list_only = 0;
    int i;

    for (i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--list") == 0) {
            list_only = 1;
        } else if (strncmp(argv[i], "--filter=", 9) == 0) {
            filter = argv[i] + 9;
        } else if (strcmp(argv[i], "--filter") == 0 && i + 1 < argc) {
            filter = argv[++i];
        }
    }

    if (list_only) {
        yb_case* c;
        for (c = yb_head; c != NULL; c = c->next) {
            if (yb_match(c->name, filter)) {
                printf("%s\n", c->name);
            }
        }
        return 0;
    }

    {
        int total = 0;
        int idx = 0;
        int passed = 0;
        int failed = 0;
        yb_case* c;

        for (c = yb_head; c != NULL; c = c->next) {
            if (yb_match(c->name, filter)) {
                total++;
            }
        }

        printf(YB_PROTOCOL "{\"event\":\"suite_start\",\"total\":%d}\n", total);
        fflush(stdout);

        for (c = yb_head; c != NULL; c = c->next) {
            clock_t started;
            double ms;

            if (!yb_match(c->name, filter)) {
                continue;
            }
            idx++;

            yb_case_failed = 0;
            yb_fail_file = NULL;
            yb_fail_line = 0;
            yb_fail_expr = NULL;
            yb_fail_detail[0] = '\0';

            printf(YB_PROTOCOL "{\"event\":\"case_start\",\"index\":%d,\"total\":%d,\"name\":",
                   idx, total);
            yb_put_json(c->name);
            printf("}\n");
            fflush(stdout);

            started = clock();
#ifdef __cplusplus
            try {
                c->fn();
            } catch (const std::exception& e) {
                yb_record_fail("", 0, "未捕获的 C++ 异常", e.what());
            } catch (...) {
                yb_record_fail("", 0, "未捕获的 C++ 异常", "unknown");
            }
#else
            c->fn();
#endif
            ms = (double)(clock() - started) * 1000.0 / (double)CLOCKS_PER_SEC;

            if (yb_case_failed) {
                failed++;
            } else {
                passed++;
            }

            printf(YB_PROTOCOL "{\"event\":\"case_end\",\"name\":");
            yb_put_json(c->name);
            printf(",\"ok\":%s,\"ms\":%.3f", yb_case_failed ? "false" : "true", ms);
            if (yb_case_failed) {
                printf(",\"file\":");
                yb_put_json(yb_fail_file != NULL ? yb_fail_file : "");
                printf(",\"line\":%d,\"expr\":", yb_fail_line);
                yb_put_json(yb_fail_expr != NULL ? yb_fail_expr : "");
                printf(",\"detail\":");
                yb_put_json(yb_fail_detail);
            }
            printf("}\n");
            fflush(stdout);
        }

        printf(YB_PROTOCOL
               "{\"event\":\"suite_end\",\"total\":%d,\"passed\":%d,\"failed\":%d}\n",
               passed + failed, passed, failed);
        fflush(stdout);

        return failed > 0 ? 1 : 0;
    }
}

#ifdef __cplusplus
}
#endif

#endif /* YB_IMPLEMENTATION */

#endif /* YUBILD_TEST_H */
