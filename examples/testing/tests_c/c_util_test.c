#include "yubild_test.h"

/* 纯 C：验证测试库在 C11 下能编译、能断言 */
static int twice(int v) {
    return v * 2;
}

YB_TEST(c_double_works) {
    YB_CHECK_EQ(twice(21), 42);
}

YB_TEST(c_string_eq) {
    const char* name = "yubild";
    YB_CHECK_EQ(name, "yubild");
}

/* YB_REQUIRE 通过后继续执行后续断言 */
YB_TEST(c_require_passes) {
    int v = 7;
    YB_REQUIRE(v > 0);
    YB_CHECK_EQ(v, 7);
}
