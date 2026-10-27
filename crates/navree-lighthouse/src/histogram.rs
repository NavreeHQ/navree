//! [`Histogram`]: a mergeable latency histogram with geometric buckets.

use std::collections::BTreeMap;

/// Buckets grow by 25%: each covers durations up to `FIRST * GROWTH^i`
/// milliseconds, so a percentile is read within about 12% of the truth.
const FIRST: f64 = 0.5;
const GROWTH: f64 = 1.25;
/// The last bucket holds everything above `FIRST * GROWTH^(BUCKETS-2)`
/// (about 30 minutes).
const BUCKETS: usize = 66;

/// A latency histogram: counts per geometric bucket, the count and the
/// maximum. Histograms of different minutes, processes or routes merge by
/// adding their counts, so percentiles over any period come from every
/// recorded duration, not from samples.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Histogram {
    counts: BTreeMap<u16, u64>,
    count: u64,
    max_ms: f64,
}

impl Histogram {
    /// An empty histogram.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one duration in milliseconds.
    pub fn record(&mut self, ms: f64) {
        let ms = if ms.is_finite() { ms.max(0.0) } else { 0.0 };
        *self.counts.entry(bucket_of(ms)).or_default() += 1;
        self.count += 1;
        if ms > self.max_ms {
            self.max_ms = ms;
        }
    }

    /// Adds `other`'s counts.
    pub fn merge(&mut self, other: &Histogram) {
        for (bucket, count) in &other.counts {
            *self.counts.entry(*bucket).or_default() += count;
        }
        self.count += other.count;
        if other.max_ms > self.max_ms {
            self.max_ms = other.max_ms;
        }
    }

    /// How many durations were recorded.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The `q` quantile (0.95 for p95) in milliseconds: the upper bound of
    /// the bucket it falls in, never above the maximum; 0 when empty.
    pub fn quantile(&self, q: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let rank = ((q.clamp(0.0, 1.0) * self.count as f64).ceil() as u64).max(1);
        let mut seen = 0;
        for (bucket, count) in &self.counts {
            seen += count;
            if seen >= rank {
                return upper_bound(*bucket).min(self.max_ms);
            }
        }
        self.max_ms
    }

    /// The compact text form stored in `lighthouse_aggregates.histogram`:
    /// `bucket:count` pairs separated by commas.
    pub fn encode(&self) -> String {
        self.counts
            .iter()
            .map(|(bucket, count)| format!("{bucket}:{count}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Reads [`encode`](Self::encode)'s form, with the row's maximum.
    /// Malformed pairs are skipped.
    pub fn decode(text: &str, max_ms: f64) -> Histogram {
        let mut histogram = Histogram {
            max_ms,
            ..Histogram::default()
        };
        for pair in text.split(',') {
            let Some((bucket, count)) = pair.split_once(':') else {
                continue;
            };
            let (Ok(bucket), Ok(count)) = (bucket.trim().parse::<u16>(), count.trim().parse::<u64>())
            else {
                continue;
            };
            *histogram.counts.entry(bucket).or_default() += count;
            histogram.count += count;
        }
        histogram
    }
}

/// The bucket of a duration.
fn bucket_of(ms: f64) -> u16 {
    if ms <= FIRST {
        return 0;
    }
    let index = ((ms / FIRST).ln() / GROWTH.ln()).ceil() as usize;
    index.min(BUCKETS - 1) as u16
}

/// The largest duration in a bucket.
fn upper_bound(bucket: u16) -> f64 {
    if usize::from(bucket) >= BUCKETS - 1 {
        return f64::INFINITY;
    }
    FIRST * GROWTH.powi(i32::from(bucket))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_are_close_to_the_truth() {
        let mut h = Histogram::new();
        for ms in 1..=1000 {
            h.record(f64::from(ms));
        }
        assert_eq!(h.count(), 1000);
        let p50 = h.quantile(0.5);
        let p95 = h.quantile(0.95);
        let p99 = h.quantile(0.99);
        assert!((500.0..=500.0 * 1.25).contains(&p50), "{p50}");
        assert!((950.0..=1000.0).contains(&p95), "{p95}");
        assert!((990.0..=1000.0).contains(&p99), "{p99}");
        assert_eq!(h.quantile(1.0), 1000.0);
        assert_eq!(Histogram::new().quantile(0.95), 0.0);
    }

    #[test]
    fn histograms_merge_and_round_trip() {
        let mut a = Histogram::new();
        let mut b = Histogram::new();
        for ms in [3.0, 40.0, 41.0] {
            a.record(ms);
        }
        for ms in [1200.0, 0.1] {
            b.record(ms);
        }
        let mut merged = a.clone();
        merged.merge(&b);
        assert_eq!(merged.count(), 5);
        assert_eq!(merged.quantile(1.0), 1200.0);
        let decoded = Histogram::decode(&merged.encode(), 1200.0);
        assert_eq!(decoded, merged);
        assert_eq!(Histogram::decode("x,1:2,bad:3", 4.0).count(), 2);
    }

    #[test]
    fn huge_and_odd_values_land_in_the_edge_buckets() {
        let mut h = Histogram::new();
        h.record(f64::NAN);
        h.record(-5.0);
        h.record(1e12);
        assert_eq!(h.count(), 3);
        assert_eq!(h.quantile(1.0), 1e12);
        assert_eq!(h.quantile(0.1), FIRST);
    }
}
