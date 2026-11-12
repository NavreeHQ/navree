/* Drives the core through its C interface, as a client written in C would.
 * Built and run by tests/c/run.sh against the static library. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "navree_core.h"

static NavreeBytes send(NavreeCore *core, const char *event) {
    return navree_core_send(core, (const uint8_t *)event, strlen(event));
}

static int holds(NavreeBytes bytes, const char *text) {
    size_t wanted = strlen(text);
    if (bytes.ptr == NULL || bytes.len < wanted) {
        return 0;
    }
    for (size_t at = 0; at + wanted <= bytes.len; at++) {
        if (memcmp(bytes.ptr + at, text, wanted) == 0) {
            return 1;
        }
    }
    return 0;
}

static void expect(int ok, const char *what) {
    if (!ok) {
        fprintf(stderr, "failed: %s\n", what);
        exit(1);
    }
    printf("ok: %s\n", what);
}

int main(void) {
    const char *config =
        "{\"origin\":\"https://shop.example\","
        "\"client\":{\"platform\":\"linux\",\"form\":\"desktop\",\"vocab\":1,\"text_percent\":100}}";

    expect(navree_core_new((const uint8_t *)"{", 1) == NULL, "a configuration that does not read makes no core");

    NavreeCore *core = navree_core_new((const uint8_t *)config, strlen(config));
    expect(core != NULL, "a core is made");

    NavreeBytes update = send(core, "{\"t\":\"start\"}");
    expect(holds(update, "\"t\":\"fetch\""), "starting asks for a fetch");
    expect(holds(update, "https://shop.example/_navree/native/shell"), "of the shell");
    navree_bytes_free(update);

    const char *shell =
        "{\"t\":\"response\",\"request\":1,\"status\":200,"
        "\"content_type\":\"application/vnd.navree.native+json\","
        "\"body\":\"{\\\"navree\\\":1,\\\"build\\\":\\\"b1\\\",\\\"name\\\":\\\"Charkha Supply Co.\\\",\\\"home\\\":\\\"/\\\"}\"}";
    update = send(core, shell);
    expect(holds(update, "\"t\":\"nav\""), "the shell changes where the person is");
    expect(holds(update, "\"request\":2"), "and the first page is asked for");
    navree_bytes_free(update);

    const char *query = "{\"part\":\"nav\"}";
    NavreeBytes nav = navree_core_view(core, (const uint8_t *)query, strlen(query));
    expect(holds(nav, "\"stage\":\"ready\""), "the app is ready");
    expect(holds(nav, "Charkha Supply Co."), "with one destination named after the app");
    navree_bytes_free(nav);

    NavreeBytes all = navree_core_view(core, NULL, 0);
    expect(holds(all, "\"screens\""), "the whole view state reads");
    navree_bytes_free(all);

    NavreeBytes bad = send(core, "{\"t\":\"teleport\"}");
    expect(holds(bad, "\"error\""), "an event that does not read is an error, not a crash");
    navree_bytes_free(bad);

    navree_bytes_free((NavreeBytes){NULL, 0});
    navree_core_free(core);
    navree_core_free(NULL);
    printf("done\n");
    return 0;
}
