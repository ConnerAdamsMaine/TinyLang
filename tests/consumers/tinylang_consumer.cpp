#include "tinylang.h"

#include <cstring>

int main() {
    if (tinylang_abi_version() != TINYLANG_ABI_VERSION) {
        return 10;
    }

    char *response = tinylang_compile_source_json("print 7");
    if (response == nullptr || std::strstr(response, "\"ok\":true") == nullptr) {
        tinylang_free_string(response);
        return 11;
    }
    tinylang_free_string(response);
    return 0;
}
