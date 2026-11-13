import yubild

yubild.project(name="testing", version="0.1.0", cxx_std=17)

# 被测代码
yubild.static_library(
    "mathx",
    sources=["lib/src/*.cpp"],
    public_includes=["lib/include"],
)

# C++ 测试：链接被测库
yubild.test("mathx_tests", sources=["tests/*.cpp"], deps=["mathx"])

# C 测试：纯 C，验证测试库在 C11 下同样可用
yubild.test("c_tests", sources=["tests_c/*.c"])
