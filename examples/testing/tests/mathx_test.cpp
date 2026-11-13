#include "mathx/mathx.h"
#include "yubild_test.h"

#include <string>

YB_TEST(add_works) {
    YB_CHECK_EQ(mathx::add(2, 3), 5);
    YB_CHECK_EQ(mathx::add(-4, 4), 0);
}

// 边界值两侧都要测，否则 clamp 的 < / > 变异体会存活
YB_TEST(clamp_below) {
    YB_CHECK_EQ(mathx::clamp(-1, 0, 5), 0);
}

YB_TEST(clamp_above) {
    YB_CHECK_EQ(mathx::clamp(10, 0, 5), 5);
}

YB_TEST(clamp_inside) {
    YB_CHECK_EQ(mathx::clamp(3, 0, 5), 3);
}

YB_TEST(is_even_true) {
    YB_CHECK(mathx::is_even(4));
}

YB_TEST(is_even_false) {
    YB_CHECK(!mathx::is_even(3));
}

// 故意保留一个宽松断言，用来演示变异测试报告「存活」的变异体
YB_TEST(add_lots_of_cases) {
    for (int i = 0; i < 5; ++i) {
        YB_CHECK(mathx::add(i, 0) >= 0);
    }
}
