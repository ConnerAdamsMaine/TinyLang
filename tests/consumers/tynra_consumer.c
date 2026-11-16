#include "tynra.h"

#include <stdio.h>

int main(void) {
    if (tynra_abi_version() != TYNRA_ABI_VERSION) {
        return 10;
    }

    char *response = tynra_run_source_json("print 42", "vm", NULL);
    if (response == NULL) {
        return 11;
    }
    puts(response);
    tynra_free_string(response);
    tynra_free_string(NULL);
    return 0;
}
