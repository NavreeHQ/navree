//! Fewer points than pixels: largest-triangle-three-buckets for lines, and
//! neighbours merged for bars.

/// The indices of the points to keep of a series, at most `most`, by
/// largest-triangle-three-buckets: the first and the last point always, and from each
/// bucket between the point that makes the largest triangle with its neighbours. A
/// series' shape, its peaks and its dips survive.
///
/// `xs` must be in order. `ys` holds finite values.
///
/// ```
/// use navree_ui::charts::lttb;
///
/// let xs: Vec<f64> = (0..1000).map(f64::from).collect();
/// let ys: Vec<f64> = xs.iter().map(|x| if *x == 500.0 { 100.0 } else { 0.0 }).collect();
/// let kept = lttb(&xs, &ys, 20);
/// assert_eq!(kept.len(), 20);
/// assert_eq!((kept[0], kept[19]), (0, 999));
/// assert!(kept.contains(&500), "the peak stays");
/// ```
pub fn lttb(xs: &[f64], ys: &[f64], most: usize) -> Vec<usize> {
    let n = xs.len().min(ys.len());
    if n <= most || n <= 2 {
        return (0..n).collect();
    }
    if most < 3 {
        return if most == 0 {
            Vec::new()
        } else {
            vec![0, n - 1][..most.min(2)].to_vec()
        };
    }
    // Areas are compared, never shown: put both axes on 0..1 so no product overflows.
    let span = |values: &[f64]| {
        let (lo, hi) = values[..n]
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let size = hi - lo;
        (
            lo,
            if size > 0.0 && size.is_finite() {
                size
            } else {
                1.0
            },
        )
    };
    let (x0, xw) = span(xs);
    let (y0, yh) = span(ys);
    let x = |i: usize| (xs[i] - x0) / xw;
    let y = |i: usize| (ys[i] - y0) / yh;
    let every = (n - 2) as f64 / (most - 2) as f64;
    let mut out = Vec::with_capacity(most);
    out.push(0);
    let mut a = 0;
    for bucket in 0..most - 2 {
        let next_start = (((bucket + 1) as f64 * every).floor() as usize + 1).min(n - 1);
        let next_end = (((bucket + 2) as f64 * every).floor() as usize + 1).min(n);
        let (mut ax, mut ay) = (0.0, 0.0);
        let count = next_end.saturating_sub(next_start).max(1);
        for j in next_start..next_end.max(next_start + 1).min(n) {
            ax += x(j);
            ay += y(j);
        }
        ax /= count as f64;
        ay /= count as f64;
        let start = ((bucket as f64 * every).floor() as usize + 1).min(n - 1);
        let end = (((bucket + 1) as f64 * every).floor() as usize + 1).clamp(start + 1, n);
        let (pa_x, pa_y) = (x(a), y(a));
        let mut best = start;
        let mut best_area = -1.0;
        for j in start..end {
            let area = ((pa_x - ax) * (y(j) - pa_y) - (pa_x - x(j)) * (ay - pa_y)).abs();
            if area > best_area {
                best_area = area;
                best = j;
            }
        }
        out.push(best);
        a = best;
    }
    out.push(n - 1);
    out.dedup();
    out
}

/// The rows to keep of a table of series that share their x, at most about twice `most`.
///
/// Each series keeps its highest and lowest value, and a line never runs across a gap:
/// between two kept values of a series with a gap between them, the gap's first row is
/// kept too, so the line breaks there. Series with long runs are reduced by [`lttb`]
/// run by run. When gaps are so many that runs can't be kept that way, the rows are cut
/// into equal buckets and each keeps, per series, its lowest and highest value.
pub(crate) fn rows_to_keep(xs: &[f64], series: &[&[Option<f64>]], most: usize) -> Vec<usize> {
    let n = xs.len();
    if n <= most {
        return (0..n).collect();
    }
    let value = |values: &[Option<f64>], at: usize| values.get(at).copied().flatten();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let share = (most / series.len().max(1)).max(3);
    for values in series {
        let mut at = 0;
        while at < n {
            if value(values, at).is_none() {
                keep[at] = true;
                while at < n && value(values, at).is_none() {
                    at += 1;
                }
                continue;
            }
            let start = at;
            while at < n && value(values, at).is_some() {
                at += 1;
            }
            let run_x = &xs[start..at];
            let run_y: Vec<f64> = (start..at).filter_map(|i| value(values, i)).collect();
            let budget = ((share as f64) * (at - start) as f64 / n as f64).ceil() as usize;
            for i in lttb(run_x, &run_y, budget.max(2)) {
                keep[start + i] = true;
            }
        }
    }
    if keep.iter().filter(|k| **k).count() > most * 2 {
        // Too many gaps for runs: the lowest and highest of each series in each bucket.
        keep.iter_mut().for_each(|k| *k = false);
        keep[0] = true;
        keep[n - 1] = true;
        let buckets = (most / (2 * series.len().max(1))).max(8);
        let size = n.div_ceil(buckets).max(1);
        for values in series {
            for first in (0..n).step_by(size) {
                let rows = first..(first + size).min(n);
                let mut low: Option<(usize, f64)> = None;
                let mut high: Option<(usize, f64)> = None;
                for at in rows {
                    let Some(v) = value(values, at) else { continue };
                    if low.is_none_or(|(_, l)| v < l) {
                        low = Some((at, v));
                    }
                    if high.is_none_or(|(_, h)| v > h) {
                        high = Some((at, v));
                    }
                }
                for (at, _) in low.into_iter().chain(high) {
                    keep[at] = true;
                }
            }
        }
    }
    // Each series' extremes over all its values, each in place of the nearest kept value
    // that is neither an end nor an extreme, so a line with no gap keeps as many rows as
    // it had.
    let mut placed = vec![false; n];
    placed[0] = true;
    placed[n - 1] = true;
    for values in series {
        let mut low: Option<(usize, f64)> = None;
        let mut high: Option<(usize, f64)> = None;
        for at in 0..n {
            let Some(v) = value(values, at) else { continue };
            if low.is_none_or(|(_, l)| v < l) {
                low = Some((at, v));
            }
            if high.is_none_or(|(_, h)| v > h) {
                high = Some((at, v));
            }
        }
        for (at, _) in low.into_iter().chain(high) {
            if !keep[at] {
                let gives = |i: usize| keep[i] && !placed[i] && value(values, i).is_some();
                let mut step = 1;
                let give = loop {
                    let left = at.checked_sub(step);
                    let right = (at + step < n).then_some(at + step);
                    if left.is_none() && right.is_none() {
                        break None;
                    }
                    if let Some(i) = left.filter(|i| gives(*i)) {
                        break Some(i);
                    }
                    if let Some(i) = right.filter(|i| gives(*i)) {
                        break Some(i);
                    }
                    step += 1;
                };
                if let Some(i) = give {
                    keep[i] = false;
                }
                keep[at] = true;
            }
            placed[at] = true;
        }
    }
    // No line across a gap: the first missing row between two kept values of a series
    // stays. A row kept for one series is a value of another, so go again until no
    // series needs another row.
    loop {
        let mut added = false;
        for values in series {
            let mut last: Option<usize> = None;
            let mut gap: Option<usize> = None;
            for at in 0..n {
                match value(values, at) {
                    None if keep[at] => {
                        last = None;
                        gap = None;
                    }
                    None => {
                        gap = gap.or(Some(at));
                    }
                    Some(_) if keep[at] => {
                        if let (Some(_), Some(g)) = (last, gap) {
                            keep[g] = true;
                            added = true;
                        }
                        last = Some(at);
                        gap = None;
                    }
                    Some(_) => {}
                }
            }
        }
        if !added {
            break;
        }
    }
    (0..n).filter(|i| keep[*i]).collect()
}

/// Bands of rows merged so at most `most` remain: consecutive rows, as even as can be.
pub(crate) fn merge_ranges(n: usize, most: usize) -> Vec<std::ops::Range<usize>> {
    if n <= most || most == 0 {
        return (0..n).map(|i| i..i + 1).collect();
    }
    let size = n.div_ceil(most);
    (0..n)
        .step_by(size)
        .map(|start| start..(start + size).min(n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_series_are_kept_whole() {
        assert_eq!(lttb(&[1.0, 2.0], &[3.0, 4.0], 10), [0, 1]);
        assert_eq!(lttb(&[], &[], 10), Vec::<usize>::new());
        assert_eq!(lttb(&[0.0, 1.0, 2.0, 3.0], &[0.0; 4], 2), [0, 3]);
        assert_eq!(
            lttb(&[0.0, 1.0, 2.0, 3.0], &[0.0; 4], 0),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn peaks_and_dips_survive() {
        let n = 10_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let ys: Vec<f64> = (0..n)
            .map(|i| match i {
                1234 => 1e300,
                8765 => -1e300,
                _ => (i as f64 / 50.0).sin(),
            })
            .collect();
        let kept = lttb(&xs, &ys, 200);
        assert!(kept.len() <= 200);
        assert!(kept.contains(&1234) && kept.contains(&8765), "{kept:?}");
        assert!(kept.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_line_with_no_gap_keeps_its_count_and_its_extremes() {
        let n = 50_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        // A peak and a dip LTTB alone could pass over: one row each, in a busy wave.
        let a: Vec<Option<f64>> = (0..n)
            .map(|i| {
                Some(match i {
                    31_337 => 2.5,
                    4_242 => -2.5,
                    _ => ((i as f64) / 7.0).sin() * 2.0,
                })
            })
            .collect();
        let alone = lttb(
            &xs,
            &a.iter().map(|v| v.unwrap_or(0.0)).collect::<Vec<_>>(),
            640,
        )
        .len();
        let kept = rows_to_keep(&xs, &[&a], 640);
        assert_eq!(
            kept.len(),
            alone,
            "as many rows as largest-triangle-three-buckets chose"
        );
        assert!(kept.len() <= 640);
        assert!(kept.contains(&31_337) && kept.contains(&4_242));
        assert_eq!((kept[0], kept[kept.len() - 1]), (0, n - 1));
    }

    #[test]
    fn rows_keep_gaps_and_every_series() {
        let n = 5_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let a: Vec<Option<f64>> = (0..n)
            .map(|i| {
                if (2000..2100).contains(&i) {
                    None
                } else {
                    Some((i % 97) as f64)
                }
            })
            .collect();
        let b: Vec<Option<f64>> = (0..n)
            .map(|i| Some(if i == 4321 { 1e9 } else { 1.0 }))
            .collect();
        let kept = rows_to_keep(&xs, &[&a, &b], 300);
        assert!(kept.len() <= 600, "{}", kept.len());
        assert!(kept.contains(&2000), "the gap's first row");
        assert!(kept.contains(&4321), "the other series' peak");
        assert_eq!((kept[0], *kept.last().unwrap()), (0, n - 1));
    }

    /// Whether a line drawn through the kept rows of `values` runs across a gap.
    fn crosses_a_gap(values: &[Option<f64>], kept: &[usize]) -> Option<(usize, usize)> {
        let mut last: Option<usize> = None;
        for &at in kept {
            match values[at] {
                None => last = None,
                Some(_) => {
                    if let Some(before) = last
                        && values[before + 1..at].iter().any(Option::is_none)
                    {
                        return Some((before, at));
                    }
                    last = Some(at);
                }
            }
        }
        None
    }

    #[test]
    fn gaps_everywhere_keep_peaks_and_break_lines() {
        // A gap every ten rows and a spike in a run: every run is short.
        let n = 20_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let a: Vec<Option<f64>> = (0..n)
            .map(|i| match i {
                _ if i % 10 == 9 => None,
                12_345 => Some(1e6),
                _ => Some((i % 10) as f64),
            })
            .collect();
        let kept = rows_to_keep(&xs, &[&a], 640);
        assert!(kept.len() <= 2 * 640, "{}", kept.len());
        assert!(kept.contains(&12_345), "the spike");
        assert_eq!(crosses_a_gap(&a, &kept), None);
        // Alternating values and gaps, with a dip and a peak, in two series.
        let b: Vec<Option<f64>> = (0..n)
            .map(|i| (i % 2 == 0).then_some(if i == 7_000 { -5.0 } else { 1.0 }))
            .collect();
        let c: Vec<Option<f64>> = (0..n)
            .map(|i| (i % 3 != 0).then_some(if i == 15_001 { 9.0 } else { 2.0 }))
            .collect();
        let kept = rows_to_keep(&xs, &[&b, &c], 100);
        assert!(kept.len() <= 2 * 100 + 8, "{}", kept.len());
        assert!(
            kept.contains(&7_000) && kept.contains(&15_001),
            "each series' extremes"
        );
        assert_eq!(crosses_a_gap(&b, &kept), None);
        assert_eq!(crosses_a_gap(&c, &kept), None);
        // Short runs with a peak in the middle and gaps between, under the old stride limit.
        let n = 3_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let d: Vec<Option<f64>> = (0..n)
            .map(|i| match i % 6 {
                5 => None,
                _ if i == 1_202 => Some(500.0),
                _ => Some(1.0),
            })
            .collect();
        let kept = rows_to_keep(&xs, &[&d], 300);
        assert!(kept.contains(&1_202));
        assert_eq!(crosses_a_gap(&d, &kept), None);
    }

    #[test]
    fn bands_merge_evenly() {
        assert_eq!(merge_ranges(3, 10), [0..1, 1..2, 2..3]);
        let ranges = merge_ranges(1000, 100);
        assert_eq!(ranges.len(), 100);
        assert_eq!(ranges.iter().map(|r| r.len()).sum::<usize>(), 1000);
        assert_eq!(merge_ranges(7, 3), [0..3, 3..6, 6..7]);
    }
}
