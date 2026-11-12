/*
 * navree_core.h
 *
 * The C interface of Navree Native's runtime core. Five functions; everything
 * that crosses is UTF-8 JSON, passed as a pointer and a length (no ending zero).
 * The JSON forms are in the crate's README and in
 * schema/navree-native-core.schema.json.
 *
 * A client:
 *   1. makes one core with navree_core_new, from its configuration;
 *   2. tells it what happened with navree_core_send, one event a call, in the
 *      order things happened, and gets back an update: the parts of the view
 *      state that changed, and effects to carry out;
 *   3. reads each changed part with navree_core_view and draws from it;
 *   4. carries out each effect and sends what came of it as a later event;
 *   5. gives every NavreeBytes back with navree_bytes_free, and ends with
 *      navree_core_free.
 *
 * Threads: call from one thread at a time, in order. The simplest way is to
 * call only from the thread that draws. A lock inside the handle keeps two
 * calls made at once from running together, so a mistake costs order, never
 * memory; it does not make the order of two racing events meaningful. The
 * thread needs about 1 MiB of stack.
 *
 * Errors: a call never crashes on bad input. navree_core_new answers NULL for
 * a configuration it can't read. navree_core_send and navree_core_view answer
 * {"error":"..."} for an event or a query they can't read, for a NULL core,
 * and for a failure inside the core, after which the core answers only errors.
 */

#ifndef NAVREE_CORE_H
#define NAVREE_CORE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* A core. Made by navree_core_new, ended by navree_core_free. */
typedef struct NavreeCore NavreeCore;

/* Bytes the core owns: `len` bytes of UTF-8 JSON at `ptr`. An empty answer
 * has a NULL `ptr` and a `len` of 0. Give each back exactly once. */
typedef struct NavreeBytes {
    uint8_t *ptr;
    size_t len;
} NavreeBytes;

/* Makes a core from a configuration:
 *   {"origin":"https://shop.example",
 *    "client":{"platform":"ios","form":"phone","vocab":1,"text_percent":100}}
 * NULL when the configuration does not read. The bytes are copied. */
NavreeCore *navree_core_new(const uint8_t *config, size_t len);

/* Tells the core what happened: one event, such as {"t":"start"}.
 * Answers the update: {"changed":[...],"effects":[...]}. */
NavreeBytes navree_core_send(NavreeCore *core, const uint8_t *event, size_t len);

/* Reads a part of the view state: a query, such as {"part":"nav"} or
 * {"part":"screen","id":3}. A NULL query, or a length of 0, reads all of it. */
NavreeBytes navree_core_view(const NavreeCore *core, const uint8_t *query, size_t len);

/* Gives bytes back to the core. */
void navree_bytes_free(NavreeBytes bytes);

/* Ends a core. NULL is allowed. No other call may be using it. */
void navree_core_free(NavreeCore *core);

#ifdef __cplusplus
}
#endif

#endif /* NAVREE_CORE_H */
