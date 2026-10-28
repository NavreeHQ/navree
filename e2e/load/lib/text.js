// A plain-text end-of-test report (no network: the harness runs k6 offline,
// so jslib's k6-summary is not fetched).

const fmt = (v) => (typeof v === 'number' ? (Number.isInteger(v) ? String(v) : v.toFixed(2)) : String(v));

/** Every metric with its values, thresholds marked ok or FAIL. */
export function textSummary(data) {
  const lines = [];
  const names = Object.keys(data.metrics).sort();
  for (const name of names) {
    const m = data.metrics[name];
    const values = Object.entries(m.values)
      .map(([k, v]) => `${k}=${fmt(v)}`)
      .join(' ');
    lines.push(`${name.padEnd(48)} ${values}`);
    if (m.thresholds) {
      for (const [expr, result] of Object.entries(m.thresholds)) {
        lines.push(`  ${result.ok ? 'ok  ' : 'FAIL'} ${expr}`);
      }
    }
  }
  if (data.root_group && data.root_group.checks) {
    lines.push('');
    for (const c of data.root_group.checks) {
      lines.push(`check ${c.passes}/${c.passes + c.fails} ${c.name}`);
    }
  }
  return `${lines.join('\n')}\n`;
}
