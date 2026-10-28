// navree-realtime: publishes through `POST /api/v1/live/publish` (the
// workspace's private channel) and, in `roundTripStep`, a Pusher-protocol
// WebSocket client that authorizes the channel at `/broadcasting/auth`,
// subscribes, and times the publish until the event arrives
// (`realtime_delivery_ms`). With `realtime.backplane = "redis"` the event
// may be published by one process and delivered by another.

import http from 'k6/http';
import ws from 'k6/ws';
import { check } from 'k6';
import { Rate, Trend } from 'k6/metrics';
import { signInPool, sessionFor, headers } from '../lib/auth.js';
import { BASE_URL, REALTIME_KEY, WORKSPACE } from '../lib/config.js';
import { json, post } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { summaryFor } from '../lib/summary.js';

export const delivery = new Trend('realtime_delivery_ms', true);
export const delivered = new Rate('realtime_delivered');

/** One publish to the workspace's channel; returns the channel's name. */
export function publishStep(session) {
  const res = post(session, '/v1/live/publish', 'POST /api/v1/live/publish', {
    event: 'load.ping',
    data: { vu: __VU, iter: __ITER, at: Date.now() },
  });
  check(res, { 'publish: 200': (r) => r.status === 200 });
  return json(res, 'channel');
}

/**
 * Connects, subscribes to the private channel and waits for a publish to
 * arrive (at most 10 s). One connection per call: run it from a few
 * virtual users, not every iteration of a heavy profile.
 */
export function roundTripStep(session, channel) {
  const url = `${BASE_URL.replace(/^http/, 'ws')}/app/${REALTIME_KEY}?protocol=7&client=k6&version=1.0`;
  let sentAt = 0;
  let got = false;
  const res = ws.connect(url, {}, (socket) => {
    socket.on('message', (raw) => {
      const msg = JSON.parse(raw);
      const data = typeof msg.data === 'string' && msg.data.startsWith('{') ? JSON.parse(msg.data) : msg.data;
      if (msg.event === 'pusher:connection_established') {
        const auth = http.post(
          `${BASE_URL}/broadcasting/auth`,
          JSON.stringify({ socket_id: data.socket_id, channel_name: channel }),
          { headers: headers(session.token, session.workspace), tags: { name: 'POST /broadcasting/auth' } },
        );
        check(auth, { 'channel auth: 200': (r) => r.status === 200 });
        socket.send(JSON.stringify({ event: 'pusher:subscribe', data: { channel, auth: json(auth, 'auth') } }));
      } else if (msg.event === 'pusher_internal:subscription_succeeded') {
        sentAt = Date.now();
        post(session, '/v1/live/publish', 'POST /api/v1/live/publish', {
          event: 'load.roundtrip',
          data: { vu: __VU, iter: __ITER },
        });
      } else if (msg.event === 'load.roundtrip' && data && data.vu === __VU) {
        delivery.add(Date.now() - sentAt);
        got = true;
        socket.close();
      }
    });
    socket.setTimeout(() => socket.close(), 10000);
  });
  check(res, { 'websocket: 101': (r) => r && r.status === 101 });
  delivered.add(got);
}

export const options = profileOptions({
  extra: { realtime_delivered: ['rate>0.99'], realtime_delivery_ms: ['p(95)<1000'] },
});

export function setup() {
  const pool = signInPool(Number(__ENV.POOL || 10)).map((s) => ({ token: s.token, workspace: WORKSPACE }));
  return { pool, channel: publishStep(pool[0]) };
}

export default function (data) {
  const session = sessionFor(data.pool);
  publishStep(session);
  roundTripStep(session, data.channel);
}

export const handleSummary = summaryFor('realtime');
