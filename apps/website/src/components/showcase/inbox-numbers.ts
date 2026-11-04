/**
 * What the support inbox measured at 100,000 conversations and about 5.5 million
 * messages: `e2e/ui/tests/inbox-scale.test.mjs` against a release build on SQLite, in
 * Chromium, on one developer machine. Written from the suite's numbers; not edited by hand.
 */
export const inboxNumbers = {
  "measured": "2026-11-04",
  "messages": "5,499,392",
  "seed": "28.3 s",
  "firstScreen": {
    "largest_paint_ms": 116,
    "first_paint_ms": 116,
    "ttfb_ms": 16.4,
    "server_ms": 14.1,
    "bytes_total": 85853,
    "document_bytes": 7764,
    "runs": 5,
    "dom_nodes": 1469
  },
  "openThread": {
    "largest_paint_ms": 156,
    "first_paint_ms": 156,
    "ttfb_ms": 18.1,
    "server_ms": 15.9,
    "bytes_total": 108618,
    "document_bytes": 12419,
    "runs": 5,
    "dom_nodes": 2870
  },
  "oldest": {
    "ms": 225,
    "fetches": 3,
    "bytes_uncompressed": 60745
  },
  "flat": {
    "samples": [
      {
        "seen": 250,
        "rows": 250,
        "nodes": 5684,
        "heap_kb": 2254
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
        "heap_kb": 2397
      },
      {
        "seen": 1500,
        "rows": 300,
        "nodes": 6709,
        "heap_kb": 2488
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
    "maxHeapKb": 2488
  },
  "server": {
    "list: the first page": {
      "p50_ms": 9.7,
      "p95_ms": 15.4,
      "p99_ms": 22,
      "requests": 200
    },
    "list: a page deep in the closed list": {
      "p50_ms": 9.3,
      "p95_ms": 17.7,
      "p99_ms": 24.9,
      "requests": 200
    },
    "search": {
      "p50_ms": 48.9,
      "p95_ms": 73.6,
      "p99_ms": 85.4,
      "requests": 200
    },
    "thread: the newest thirty": {
      "p50_ms": 5.7,
      "p95_ms": 15.5,
      "p99_ms": 25.6,
      "requests": 200
    },
    "thread: thirty older": {
      "p50_ms": 5.8,
      "p95_ms": 14,
      "p99_ms": 18.1,
      "requests": 200
    },
    "page: the inbox": {
      "p50_ms": 30.2,
      "p95_ms": 53.2,
      "p99_ms": 66,
      "requests": 200
    },
    "page: a conversation": {
      "p50_ms": 52.4,
      "p95_ms": 82,
      "p99_ms": 89.2,
      "requests": 200
    }
  },
  "plans": "-- list: the first page of all open\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- list: the page after a cursor, closed\n  SEARCH conversations USING INDEX conversations_inbox (inbox=? AND last_at<?)\n\n-- list: the page before a cursor, closed\n  SEARCH conversations USING INDEX conversations_inbox (inbox=? AND last_at>?)\n\n-- list: assigned to me\n  SEARCH conversations USING INDEX conversations_mine (assignee_id=? AND inbox=?)\n\n-- list: unassigned\n  SEARCH conversations USING INDEX conversations_unassigned (status=? AND assignee_id=?)\n\n-- list: closed, one channel and priority\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- search: closed, \"booking page\"\n  SEARCH conversations USING INTEGER PRIMARY KEY (rowid=?)\n  LIST SUBQUERY 1\n  SCAN conversations_fts VIRTUAL TABLE INDEX 0:M3\n  CREATE BLOOM FILTER\n  USE TEMP B-TREE FOR ORDER BY\n\n-- live: what changed since a revision\n  SEARCH conversations USING INDEX conversations_rev (rev>?)\n\n-- count: all open\n  SEARCH conversations USING INDEX conversations_inbox (inbox=?)\n\n-- count: something new in all open\n  SEARCH conversations USING INDEX conversations_unread (inbox=?)\n\n-- thread: the newest thirty\n  SEARCH messages USING INDEX messages_conversation (conversation_id=?)\n\n-- thread: thirty before a cursor\n  SEARCH messages USING INDEX messages_conversation (conversation_id=? AND id<?)\n\n-- thread: what is new since a message\n  SEARCH messages USING INDEX messages_conversation (conversation_id=? AND id>?)"
} as const;
