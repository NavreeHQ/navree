-- Prints the run's results as one JSON line for bench/run.mjs.
done = function(summary, latency, requests)
  local e = summary.errors
  io.write(string.format(
    'BENCH_RESULT {"requests":%d,"duration_us":%d,"bytes":%d,' ..
    '"errors":{"connect":%d,"read":%d,"write":%d,"status":%d,"timeout":%d},' ..
    '"latency_us":{"mean":%.1f,"stdev":%.1f,"max":%d,"p50":%d,"p75":%d,"p90":%d,"p99":%d,"p999":%d}}\n',
    summary.requests, summary.duration, summary.bytes,
    e.connect, e.read, e.write, e.status, e.timeout,
    latency.mean, latency.stdev, latency.max,
    latency:percentile(50), latency:percentile(75), latency:percentile(90),
    latency:percentile(99), latency:percentile(99.9)))
end
