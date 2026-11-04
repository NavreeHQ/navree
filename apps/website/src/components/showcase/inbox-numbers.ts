/**
 * What the support inbox measured at 100,000 conversations and about 5.5 million
 * messages: `e2e/ui/tests/inbox-scale.test.mjs` against a release build on SQLite, in
 * Chromium, on one developer machine. Written from the suite's numbers; not edited by hand.
 */
export const inboxNumbers = {
  "measured": "2026-11-04",
  "messages": "5,499,392",
  "seed": "29.9 s",
  "firstScreen": {
    "largest_paint_ms": 84,
    "first_paint_ms": 84,
    "ttfb_ms": 10.3,
    "server_ms": 9,
    "bytes_total": 87036,
    "document_bytes": 7901,
    "script_bytes": 33563,
    "scripts": [
      "core",
      "keys",
      "lazy",
      "live",
      "overlay",
      "select",
      "sidebar",
      "split"
    ],
    "runs": 5,
    "dom_nodes": 1474
  },
  "openThread": {
    "largest_paint_ms": 116,
    "first_paint_ms": 116,
    "ttfb_ms": 14.7,
    "server_ms": 13.3,
    "bytes_total": 109865,
    "document_bytes": 12911,
    "script_bytes": 47899,
    "scripts": [
      "core",
      "dates",
      "keys",
      "lazy",
      "live",
      "menus",
      "overlay",
      "select",
      "sidebar",
      "split"
    ],
    "runs": 5,
    "dom_nodes": 2873
  },
  "oldest": {
    "ms": 194,
    "fetches": 3,
    "bytes_uncompressed": 63797
  },
  "flat": {
    "samples": [
      {
        "seen": 250,
        "rows": 250,
        "nodes": 5689,
        "heap_kb": 2258
      },
      {
        "seen": 500,
        "rows": 300,
        "nodes": 6698,
        "heap_kb": 2384
      },
      {
        "seen": 750,
        "rows": 300,
        "nodes": 6713,
        "heap_kb": 2408
      },
      {
        "seen": 1000,
        "rows": 300,
        "nodes": 6725,
        "heap_kb": 2409
      },
      {
        "seen": 1250,
        "rows": 300,
        "nodes": 6732,
        "heap_kb": 2411
      },
      {
        "seen": 1500,
        "rows": 300,
        "nodes": 6714,
        "heap_kb": 2414
      },
      {
        "seen": 1750,
        "rows": 300,
        "nodes": 6705,
        "heap_kb": 2414
      },
      {
        "seen": 2000,
        "rows": 300,
        "nodes": 6713,
        "heap_kb": 2417
      }
    ],
    "maxRows": 300,
    "heldFrom": 500,
    "heldTo": 2000,
    "minNodes": 6698,
    "maxNodes": 6732,
    "minHeapKb": 2384,
    "maxHeapKb": 2417
  },
  "server": {
    "list: the first page": {
      "p50_ms": 6.4,
      "p95_ms": 8.3,
      "p99_ms": 14.6,
      "requests": 200
    },
    "list: a page deep in the closed list": {
      "p50_ms": 8.1,
      "p95_ms": 19.1,
      "p99_ms": 26.2,
      "requests": 200
    },
    "search": {
      "p50_ms": 43.8,
      "p95_ms": 74.5,
      "p99_ms": 83.5,
      "requests": 200
    },
    "list: closed, a channel and a priority that match little": {
      "p50_ms": 3.4,
      "p95_ms": 16.7,
      "p99_ms": 18,
      "requests": 200
    },
    "page: closed, api and urgent": {
      "p50_ms": 7.4,
      "p95_ms": 16.9,
      "p99_ms": 22.3,
      "requests": 200
    },
    "thread: the newest thirty": {
      "p50_ms": 5.3,
      "p95_ms": 16.8,
      "p99_ms": 19.1,
      "requests": 200
    },
    "thread: thirty older": {
      "p50_ms": 4.5,
      "p95_ms": 14.6,
      "p99_ms": 19.1,
      "requests": 200
    },
    "page: the inbox": {
      "p50_ms": 28.4,
      "p95_ms": 43.3,
      "p99_ms": 54.4,
      "requests": 200
    },
    "page: a conversation": {
      "p50_ms": 51.4,
      "p95_ms": 82.9,
      "p99_ms": 106.5,
      "requests": 200
    }
  },
  "live": {
    "window": 50,
    "tick": {
      "rows": 0,
      "bytes": 0,
      "brotli": 1
    },
    "change": {
      "rows": 1,
      "bytes": 1082,
      "brotli": 435
    }
  },
  "plans": "-- list: the first page of all open\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- list: the page after a cursor, closed\n  SEARCH conversations USING INDEX conversations_inbox (inbox=? AND last_at<?)\n\n-- list: the page before a cursor, closed\n  SEARCH conversations USING INDEX conversations_inbox (inbox=? AND last_at>?)\n\n-- list: the page after a cursor, closed, api and urgent\n  SEARCH conversations USING INDEX conversations_channel_priority (inbox=? AND channel=? AND priority=? AND last_at<?)\n\n-- list: assigned to me\n  SEARCH conversations USING INDEX conversations_mine (assignee_id=? AND inbox=?)\n\n-- list: assigned to me, chat and urgent\n  SEARCH conversations USING INDEX conversations_mine (assignee_id=? AND inbox=?)\n\n-- list: unassigned\n  SEARCH conversations USING INDEX conversations_unassigned (status=? AND assignee_id=?)\n\n-- list: unassigned, api and urgent\n  SEARCH conversations USING INDEX conversations_unassigned (status=? AND assignee_id=?)\n\n-- search: closed, \"booking page\"\n  SEARCH conversations USING INTEGER PRIMARY KEY (rowid=?)\n  LIST SUBQUERY 1\n  SCAN conversations_fts VIRTUAL TABLE INDEX 0:M3\n  CREATE BLOOM FILTER\n  USE TEMP B-TREE FOR ORDER BY\n\n-- live: what changed since a revision\n  SEARCH conversations USING INDEX conversations_rev (rev>?)\n\n-- count: all open\n  SEARCH conversations USING INDEX conversations_priority (inbox=?)\n\n-- count: something new in all open\n  SEARCH conversations USING INDEX conversations_unread (inbox=?)\n\n-- count: closed, api and urgent\n  SEARCH conversations USING INDEX conversations_channel_priority (inbox=? AND channel=? AND priority=?)\n\n-- list: all, api\n  SEARCH conversations USING INDEX conversations_channel (inbox=? AND channel=?)\n\n-- list: all, urgent\n  SEARCH conversations USING INDEX conversations_priority (inbox=? AND priority=?)\n\n-- list: all, api and urgent\n  SEARCH conversations USING INDEX conversations_channel_priority (inbox=? AND channel=? AND priority=?)\n\n-- list: snoozed, api\n  SEARCH conversations USING INDEX conversations_channel (inbox=? AND channel=?)\n\n-- list: snoozed, urgent\n  SEARCH conversations USING INDEX conversations_priority (inbox=? AND priority=?)\n\n-- list: snoozed, api and urgent\n  SEARCH conversations USING INDEX conversations_channel_priority (inbox=? AND channel=? AND priority=?)\n\n-- list: closed, api\n  SEARCH conversations USING INDEX conversations_channel (inbox=? AND channel=?)\n\n-- list: closed, urgent\n  SEARCH conversations USING INDEX conversations_priority (inbox=? AND priority=?)\n\n-- list: closed, api and urgent\n  SEARCH conversations USING INDEX conversations_channel_priority (inbox=? AND channel=? AND priority=?)\n\n-- thread: the newest thirty\n  SEARCH messages USING INDEX messages_conversation (conversation_id=?)\n\n-- thread: thirty before a cursor\n  SEARCH messages USING INDEX messages_conversation (conversation_id=? AND id<?)\n\n-- thread: what is new since a message\n  SEARCH messages USING INDEX messages_conversation (conversation_id=? AND id>?)"
} as const;
