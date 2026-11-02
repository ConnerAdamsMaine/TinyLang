#include "tinylang.h"

#include <stdio.h>

int main(void) {
    if (tinylang_abi_version() != TINYLANG_ABI_VERSION) {
        return 10;
    }

    char *response = tinylang_run_source_json("print 42", "vm", NULL);
    if (response == NULL) {
        return 11;
    }
    puts(response);
    tinylang_free_string(response);
    tinylang_free_string(NULL);
    return 0;
}
