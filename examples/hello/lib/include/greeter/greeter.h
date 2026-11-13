#pragma once

#include <string>

namespace greeter {

// 返回一句问候；刻意放在库里，用来验证 public_includes 的传播。
std::string greeting(const std::string& who);

}  // namespace greeter
