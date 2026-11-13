#include "greeter/greeter.h"

namespace greeter {

std::string greeting(const std::string& who) {
    return "hello, " + who + "!";
}

}  // namespace greeter
