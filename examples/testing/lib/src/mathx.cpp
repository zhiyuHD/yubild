#include "mathx/mathx.h"

namespace mathx {

int add(int a, int b) {
    return a + b;
}

int clamp(int value, int lo, int hi) {
    if (value < lo) {
        return lo;
    }
    if (value > hi) {
        return hi;
    }
    return value;
}

bool is_even(int value) {
    return value % 2 == 0;
}

int twice_untested(int value) {
    return value * 2;
}

}  // namespace mathx
