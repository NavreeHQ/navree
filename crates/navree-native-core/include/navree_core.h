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
 * Freeing: navree_core_free may be called while a call is in progress on
 * another thread. It waits for that call, which finishes with its answer,
 * then drops the core. A call that comes during or after the free answers
 * {"error":"the core was freed"}, and a second free does nothing: the
 * handle's own few bytes (a lock and a mark) are kept for the life of the
 * process, so no call reads memory that was given back. A client still frees
 * each core once, when it is done with it.
 *
 * Addresses: every address in an effect that a client fetches, streams from
 * or shows in the web view is a whole address, scheme://host[:port]/path?query,
 * percent-encoded as a browser sends it, on the app's own origin or one it
 * allows. Before it uses one, a client asks "is this ours?" of what its
 * platform's own parser reads: the text holds no white space, control
 * character or backslash; the platform reads it as http or https with no user
 * information; its scheme, host and port are an origin of the app. It runs the
 * `ours` list of vectors/addresses.json to prove it (the README, "Addresses").
 * An open_url effect holds only a whole http or https address, or, only when
 * the person's tap stands behind it, a mailto, tel, sms or geo one. A client
 * sends web_link only for a navigation the person started in the web view.
 *
 * Sizes: a call copies the bytes it is given before the core can look at
 * them, so the core's own refusal of a body past MAX_BODY (8 MiB, in
 * navree-native) comes after the copy. The guard that keeps a very large body
 * out of memory is the client's transport: it refuses a body larger than
 * MAX_BODY before it hands it over, by the length the response announces and
 * by the bytes that have arrived, and answers the core with a failure as for
 * any answer it could not read. What the core is handed from the device's
 * store (a snapshot, a kept page, what was kept at the last launch) it wrote
 * itself.
 *
 * Errors: a call never crashes on bad input. navree_core_new answers NULL for
 * a configuration it can't read. navree_core_send and navree_core_view answer
 * {"error":"..."} for an event or a query they can't read, for a NULL core,
 * and for a failure inside the core, which includes a failure while the call's
 * own input is read (for navree_core_new, that is a configuration that does
 * not read: NULL). After such a failure the core has stopped for good: what
 * it held is dropped, nothing it had queued is sent, and that call and every
 * later navree_core_send and navree_core_view answer
 * {"error":"...","stopped":true}. So does a core whose lock a failure left
 * poisoned. A client tests "stopped" (never the words, which may change),
 * says the app must be started again, and still frees the handle. No other
 * answer carries "stopped": an input that merely does not read is only an
 * error, and the core goes on.
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

/* Ends a core. NULL is allowed. A call in progress on another thread finishes
 * first; later calls on the handle answer an error. */
void navree_core_free(NavreeCore *core);

#ifdef __cplusplus
}
#endif

#endif /* NAVREE_CORE_H */
