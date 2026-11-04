/**
 * What the support inbox measured at 100,000 conversations and about 5.5 million
 * messages: `e2e/ui/tests/inbox-scale.test.mjs` against a release build on SQLite, in
 * Chromium, on one developer machine. Written from the suite's numbers; not edited by hand.
 */
export const inboxNumbers = {
  "measured": "2026-11-04",
  "messages": "5,499,392",
  "seed": "34.6 s",
  "firstScreen": {
    "largest_paint_ms": 136,
    "first_paint_ms": 136,
    "ttfb_ms": 20.6,
    "server_ms": 16.6,
    "bytes_total": 85869,
    "document_bytes": 7780,
    "runs": 5,
    "dom_nodes": 1469
  },
  "openThread": {
    "largest_paint_ms": 168,
    "first_paint_ms": 168,
    "ttfb_ms": 44.6,
    "server_ms": 42.5,
    "bytes_total": 110677,
    "document_bytes": 12391,
    "runs": 5,
    "dom_nodes": 2885
  },
  "oldest": {
    "ms": 211,
    "fetches": 3,
    "bytes_uncompressed": 59975
  },
  "flat": {
    "samples": [
      {
        "seen": 250,
        "rows": 250,
        "nodes": 5684,
        "heap_kb": 2252
      },
      {
        "seen": 500,
        "rows": 300,
        "nodes": 6693,
        "heap_kb": 2374
      },
      {
        "seen": 750,
        "rows": 300,
        "nodes": 6708,
        "heap_kb": 2395
      },
      {
        "seen": 1000,
        "rows": 300,
        "nodes": 6720,
        "heap_kb": 2396
      },
      {
        "seen": 1250,
        "rows": 300,
        "nodes": 6727,
        "heap_kb": 2434
      },
      {
        "seen": 1500,
        "rows": 300,
        "nodes": 6709,
        "heap_kb": 2437
      },
      {
        "seen": 1750,
        "rows": 300,
        "nodes": 6700,
        "heap_kb": 2437
      },
      {
        "seen": 2000,
        "rows": 300,
        "nodes": 6708,
        "heap_kb": 2441
      }
    ],
    "maxRows": 300,
    "heldFrom": 500,
    "heldTo": 2000,
    "minNodes": 6693,
    "maxNodes": 6727,
    "minHeapKb": 2374,
    "maxHeapKb": 2441
  },
  "server": {
    "list: the first page": {
      "p50_ms": 11,
      "p95_ms": 19.7,
      "p99_ms": 25,
      "requests": 200
    },
    "list: a page deep in the closed list": {
      "p50_ms": 11.7,
      "p95_ms": 23,
      "p99_ms": 29.4,
      "requests": 200
    },
    "search": {
      "p50_ms": 42.3,
      "p95_ms": 72.5,
      "p99_ms": 83,
      "requests": 200
    },
    "thread: the newest thirty": {
      "p50_ms": 8,
      "p95_ms": 16.8,
      "p99_ms": 19.5,
      "requests": 200
    },
    "thread: thirty older": {
      "p50_ms": 8,
      "p95_ms": 20.2,
      "p99_ms": 24.7,
      "requests": 200
    },
    "page: the inbox": {
      "p50_ms": 39.7,
      "p95_ms": 83.2,
      "p99_ms": 161.6,
      "requests": 200
    },
    "page: a conversation": {
      "p50_ms": 66.1,
      "p95_ms": 110.3,
      "p99_ms": 129.9,
      "requests": 200
    }
  },
  "plans": "-- list: the first page of all open\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- list: the page after a cursor, closed\n  SEARCH conversations USING INDEX conversations_inbox (inbox=? AND last_at<?)\n\n-- list: the page before a cursor, closed\n  SEARCH conversations USING INDEX conversations_inbox (inbox=? AND last_at>?)\n\n-- list: assigned to me\n  SEARCH conversations USING INDEX conversations_mine (assignee_id=? AND inbox=?)\n\n-- list: unassigned\n  SEARCH conversations USING INDEX conversations_unassigned (status=? AND assignee_id=?)\n\n-- list: closed, one channel and priority\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- search: closed, \"booking page\"\n  SEARCH conversations USING INTEGER PRIMARY KEY (rowid=?)\n  LIST SUBQUERY 1\n  SCAN conversations_fts VIRTUAL TABLE INDEX 0:M3\n  CREATE BLOOM FILTER\n  USE TEMP B-TREE FOR ORDER BY\n\n-- live: what changed since a revision\n  SEARCH conversations USING INDEX conversations_rev (rev>?)\n\n-- count: all open\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- count: something new in all open\n  SEARCH conversations USING INDEX conversations_unread (inbox=?)\n\n-- thread: the newest thirty\n  SEARCH messages USING INDEX messages_conversation (conversation_id=?)\n\n-- thread: thirty before a cursor\n  SEARCH messages USING INDEX messages_conversation (conversation_id=? AND id<?)\n\n-- thread: what is new since a message\n  SEARCH messages USING INDEX messages_conversation (conversation_id=? AND id>?)"
} as const;
