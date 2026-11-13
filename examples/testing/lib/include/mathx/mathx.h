#pragma once

namespace mathx {

int add(int a, int b);
int clamp(int value, int lo, int hi);
bool is_even(int value);

// 故意不给它写测试：变异测试会把这个「没被覆盖的行为」指出来
int twice_untested(int value);

}  // namespace mathx
