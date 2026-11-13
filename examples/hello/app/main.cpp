#include <iostream>
#include <string>
#include <thread>

#include "greeter/greeter.h"

int main() {
    std::string message;
    std::thread worker([&message] { message = greeter::greeting("yubild"); });
    worker.join();

    std::cout << message << "\n";
    std::cout << "C++ 标准: " << __cplusplus << "\n";
    return 0;
}
