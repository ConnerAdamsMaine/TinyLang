#include "tynra.h"

#include <cstring>

int main() {
    if (tynra_abi_version() != TYNRA_ABI_VERSION) {
        return 10;
    }

    char *response = tynra_compile_source_json("print 7");
    if (response == nullptr || std::strstr(response, "\"ok\":true") == nullptr) {
        tynra_free_string(response);
        return 11;
    }
    tynra_free_string(response);
    return 0;
}
