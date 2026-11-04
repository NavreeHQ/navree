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

/// The rows to keep of a table of series that share their x, at most about `most`:
/// each series reduced by [`lttb`] over its own runs of values, a gap kept where a
/// line breaks, and every series' picks together. When gaps alone are too many, every
/// k-th row.
pub(crate) fn rows_to_keep(xs: &[f64], series: &[&[Option<f64>]], most: usize) -> Vec<usize> {
    let n = xs.len();
    if n <= most {
        return (0..n).collect();
    }
    let share = (most / series.len().max(1)).max(3);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    for values in series {
        let mut at = 0;
        while at < n {
            if values.get(at).copied().flatten().is_none() {
                // A gap: keep its first row, so the line breaks there too.
                keep[at] = true;
                while at < n && values.get(at).copied().flatten().is_none() {
                    at += 1;
                }
                continue;
            }
            let start = at;
            while at < n && values.get(at).copied().flatten().is_some() {
                at += 1;
            }
            let run_x = &xs[start..at];
            let run_y: Vec<f64> = (start..at).filter_map(|i| values[i]).collect();
            let budget = ((share as f64) * (at - start) as f64 / n as f64).ceil() as usize;
            for i in lttb(run_x, &run_y, budget.max(2)) {
                keep[start + i] = true;
            }
        }
    }
    let kept: Vec<usize> = (0..n).filter(|i| keep[*i]).collect();
    if kept.len() <= most * 2 {
        return kept;
    }
    let stride = n.div_ceil(most).max(1);
    let mut out: Vec<usize> = (0..n).step_by(stride).collect();
    if out.last() != Some(&(n - 1)) {
        out.push(n - 1);
    }
    out
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

    #[test]
    fn gaps_everywhere_fall_back_to_a_stride() {
        let n = 10_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let a: Vec<Option<f64>> = (0..n).map(|i| (i % 2 == 0).then_some(1.0)).collect();
        let kept = rows_to_keep(&xs, &[&a], 100);
        assert!(kept.len() <= 101, "{}", kept.len());
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
