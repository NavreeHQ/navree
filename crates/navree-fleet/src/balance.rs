//! The balancer: how many processes each queue of a supervisor gets. Pure
//! functions of the supervisor, the current processes and the workload, so
//! a configuration can be checked against any workload in a test.

use crate::{Balance, QueueWorkload, Supervisor};

/// How many processes work each queue (or, with `balance = "off"`, the
/// supervisor's queues together, joined with commas), in queue order.
pub type Allocation = Vec<(String, usize)>;

/// The processes a supervisor starts with: `min_processes` per queue
/// (`auto`), `max_processes` split evenly (`simple`), or `max_processes`
/// working every queue (`off`).
pub fn initial_allocation(supervisor: &Supervisor) -> Allocation {
    match supervisor.balance {
        Balance::Auto => supervisor
            .queues
            .iter()
            .map(|queue| (queue.clone(), supervisor.min_processes))
            .collect(),
        Balance::Simple => simple(supervisor),
        Balance::Off => off(supervisor),
    }
}

/// Where the supervisor should end up for `workload`: with `auto`, every
/// queue keeps `min_processes`, and the rest of `max_processes` goes to the
/// queues in proportion to their workload (pending jobs times average
/// runtime). A queue never gets more processes than it can keep busy (its
/// waiting and running jobs over the concurrency of a process). `simple`
/// and `off` ignore the workload.
pub fn target_allocation(supervisor: &Supervisor, workload: &[QueueWorkload]) -> Allocation {
    match supervisor.balance {
        Balance::Simple => return simple(supervisor),
        Balance::Off => return off(supervisor),
        Balance::Auto => {}
    }
    let min = supervisor.min_processes;
    let concurrency = supervisor.concurrency.max(1) as u64;
    let queues = &supervisor.queues;
    // Weight and the most processes a queue can keep busy.
    let demand: Vec<(f64, usize)> = queues
        .iter()
        .map(|queue| {
            let Some(load) = workload.iter().find(|w| &w.queue == queue) else {
                return (0.0, min);
            };
            let busy = usize::try_from(load.active().div_ceil(concurrency)).unwrap_or(usize::MAX);
            (load.workload_secs.max(0.0), busy.max(min))
        })
        .collect();
    let mut targets = vec![min; queues.len()];
    let mut spare = supervisor
        .max_processes
        .saturating_sub(min.saturating_mul(queues.len()));
    // Highest averages: each spare process goes to the queue whose weight
    // per extra process is largest (earlier queues win ties).
    while spare > 0 {
        let mut best: Option<(usize, f64)> = None;
        for (i, (weight, cap)) in demand.iter().enumerate() {
            if *weight <= 0.0 || targets[i] >= *cap {
                continue;
            }
            let score = weight / ((targets[i] - min + 1) as f64);
            if best.is_none_or(|(_, top)| score > top) {
                best = Some((i, score));
            }
        }
        let Some((i, _)) = best else { break };
        targets[i] += 1;
        spare -= 1;
    }
    queues.iter().cloned().zip(targets).collect()
}

/// One balancing step from `current` towards [`target_allocation`]: each
/// queue moves by at most `max_shift` processes, never below
/// `min_processes`, and the supervisor never uses more than
/// `max_processes`. `simple` and `off` go straight to their fixed
/// allocation.
pub fn next_allocation(
    supervisor: &Supervisor,
    current: &Allocation,
    workload: &[QueueWorkload],
    max_shift: usize,
) -> Allocation {
    let target = target_allocation(supervisor, workload);
    if supervisor.balance != Balance::Auto {
        return target;
    }
    let shift = max_shift.max(1);
    let min = supervisor.min_processes;
    let now = |queue: &str| {
        current
            .iter()
            .find(|(q, _)| q == queue)
            .map_or(0, |(_, n)| *n)
    };
    let mut next: Vec<(String, usize, usize)> = target
        .iter()
        .map(|(queue, goal)| {
            let from = now(queue);
            let to = if *goal > from {
                from + (*goal - from).min(shift)
            } else {
                from - (from - *goal).min(shift)
            };
            (queue.clone(), from, to.max(min))
        })
        .collect();
    // Shrinking queues may move slower than growing ones: take back growth,
    // last queue first, until the supervisor fits its maximum.
    let mut total: usize = next.iter().map(|(_, _, n)| n).sum();
    for (_, from, to) in next.iter_mut().rev() {
        while total > supervisor.max_processes && *to > (*from).max(min) {
            *to -= 1;
            total -= 1;
        }
    }
    // Still above (the maximum was lowered): shrink the largest queues.
    while total > supervisor.max_processes {
        let Some(largest) = next
            .iter_mut()
            .filter(|(_, _, n)| *n > min)
            .max_by_key(|(_, _, n)| *n)
        else {
            break;
        };
        largest.2 -= 1;
        total -= 1;
    }
    next.into_iter().map(|(queue, _, n)| (queue, n)).collect()
}

fn simple(supervisor: &Supervisor) -> Allocation {
    let n = supervisor.queues.len().max(1);
    let (each, extra) = (supervisor.max_processes / n, supervisor.max_processes % n);
    supervisor
        .queues
        .iter()
        .enumerate()
        .map(|(i, queue)| (queue.clone(), each + usize::from(i < extra)))
        .collect()
}

fn off(supervisor: &Supervisor) -> Allocation {
    vec![(supervisor.queues.join(","), supervisor.max_processes)]
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn supervisor(balance: Balance, queues: &[&str], min: usize, max: usize) -> Supervisor {
        Supervisor {
            name: "default".into(),
            queues: queues.iter().map(|q| (*q).to_owned()).collect(),
            balance,
            min_processes: min,
            max_processes: max,
            concurrency: 4,
            max_jobs: 1000,
            max_memory_mb: 256,
            max_time_secs: 3600,
        }
    }

    fn load(queue: &str, pending: u64, avg_runtime_ms: u64) -> QueueWorkload {
        QueueWorkload::new(queue, pending, 0, 0, avg_runtime_ms)
    }

    fn counts(allocation: &Allocation) -> Vec<usize> {
        allocation.iter().map(|(_, n)| *n).collect()
    }

    #[test]
    fn auto_starts_with_the_minimum_per_queue() {
        let s = supervisor(Balance::Auto, &["default", "mail", "notifications"], 1, 8);
        assert_eq!(
            initial_allocation(&s),
            vec![
                ("default".to_owned(), 1),
                ("mail".to_owned(), 1),
                ("notifications".to_owned(), 1)
            ]
        );
    }

    #[test]
    fn simple_splits_evenly_and_off_works_every_queue() {
        let s = supervisor(Balance::Simple, &["a", "b", "c"], 1, 8);
        assert_eq!(counts(&initial_allocation(&s)), [3, 3, 2]);
        let busy = [load("a", 5000, 100)];
        assert_eq!(
            counts(&next_allocation(&s, &initial_allocation(&s), &busy, 1)),
            [3, 3, 2]
        );
        let s = supervisor(Balance::Off, &["exports", "default"], 1, 2);
        assert_eq!(
            initial_allocation(&s),
            vec![("exports,default".to_owned(), 2)]
        );
    }

    /// The docs' newsletter: 3,000 mails arrive, `mail` grows from one to
    /// six processes one step at a time while the others keep one; as it
    /// drains it returns to one.
    #[test]
    fn a_newsletter_moves_processes_to_mail_and_back() {
        let s = supervisor(Balance::Auto, &["default", "mail", "notifications"], 1, 8);
        let busy = [
            load("default", 0, 50),
            load("mail", 3000, 200),
            load("notifications", 0, 30),
        ];
        assert_eq!(counts(&target_allocation(&s, &busy)), [1, 6, 1]);
        let mut current = initial_allocation(&s);
        let mut steps = Vec::new();
        for _ in 0..6 {
            current = next_allocation(&s, &current, &busy, 1);
            steps.push(counts(&current)[1]);
        }
        assert_eq!(steps, [2, 3, 4, 5, 6, 6]);
        let drained = [load("default", 0, 50), load("mail", 0, 200)];
        for _ in 0..5 {
            current = next_allocation(&s, &current, &drained, 1);
        }
        assert_eq!(counts(&current), [1, 1, 1]);
    }

    /// Growth is sized by how many jobs wait, not by how long they take:
    /// 25 mails of 20 ms (a second of work for one process) still claim
    /// every spare process they can keep busy. They only get them if they
    /// are still waiting at the following balancing steps.
    #[test]
    fn a_small_fast_backlog_still_targets_every_spare_process_it_can_keep_busy() {
        let s = supervisor(Balance::Auto, &["default", "mail", "notifications"], 1, 8);
        let work = [load("mail", 25, 20)];
        assert_eq!(counts(&target_allocation(&s, &work)), [1, 6, 1]);
        // Seven jobs at four per process keep two processes busy.
        let work = [load("mail", 7, 20)];
        assert_eq!(counts(&target_allocation(&s, &work)), [1, 2, 1]);
    }

    #[test]
    fn spare_processes_follow_the_workload_proportionally() {
        let s = supervisor(Balance::Auto, &["a", "b"], 1, 10);
        // `a` has three times `b`'s workload: 8 spare processes split 6:2.
        let work = [load("a", 3000, 100), load("b", 1000, 100)];
        assert_eq!(counts(&target_allocation(&s, &work)), [7, 3]);
    }

    #[test]
    fn a_queue_gets_no_more_processes_than_it_can_keep_busy() {
        let s = supervisor(Balance::Auto, &["a", "b"], 1, 10);
        // Six jobs at four per process keep two processes busy.
        let work = [load("a", 6, 60_000), load("b", 0, 0)];
        assert_eq!(counts(&target_allocation(&s, &work)), [2, 1]);
    }

    #[test]
    fn a_larger_max_shift_moves_faster() {
        let s = supervisor(Balance::Auto, &["a", "b"], 1, 9);
        let work = [load("a", 5000, 100)];
        let next = next_allocation(&s, &initial_allocation(&s), &work, 3);
        assert_eq!(counts(&next), [4, 1]);
    }

    #[test]
    fn a_lowered_maximum_shrinks_the_largest_queue() {
        let s = supervisor(Balance::Auto, &["a", "b"], 1, 4);
        let current = vec![("a".to_owned(), 6), ("b".to_owned(), 2)];
        let next = next_allocation(&s, &current, &[load("a", 9000, 100)], 1);
        assert!(counts(&next).iter().sum::<usize>() <= 4, "{next:?}");
        assert!(counts(&next).iter().all(|n| *n >= 1), "{next:?}");
    }

    fn arbitrary_case() -> impl Strategy<Value = (Supervisor, Allocation, Vec<QueueWorkload>, usize)>
    {
        (1usize..6, 0usize..3, 0usize..12, 1usize..4).prop_flat_map(|(n, min, extra, shift)| {
            let queues: Vec<String> = (0..n).map(|i| format!("q{i}")).collect();
            let max = (min * n).max(1) + extra;
            (
                Just(queues),
                Just(min),
                Just(max),
                proptest::collection::vec(0usize..(max + 3), n),
                proptest::collection::vec((0u64..5000, 0u64..2000), n),
                Just(shift),
            )
                .prop_map(|(queues, min, max, current, loads, shift)| {
                    let names: Vec<&str> = queues.iter().map(String::as_str).collect();
                    let s = supervisor(Balance::Auto, &names, min, max);
                    let current: Allocation = queues.iter().cloned().zip(current).collect();
                    let work = queues
                        .iter()
                        .zip(loads)
                        .map(|(q, (pending, ms))| load(q, pending, ms))
                        .collect();
                    (s, current, work, shift)
                })
        })
    }

    proptest! {
        /// Never above the maximum, never below the minimum per queue.
        #[test]
        fn auto_stays_within_its_limits((s, current, work, shift) in arbitrary_case()) {
            let target = target_allocation(&s, &work);
            prop_assert!(counts(&target).iter().sum::<usize>() <= s.max_processes);
            prop_assert!(counts(&target).iter().all(|n| *n >= s.min_processes));
            let next = next_allocation(&s, &current, &work, shift);
            prop_assert_eq!(next.len(), s.queues.len());
            prop_assert!(counts(&next).iter().sum::<usize>() <= s.max_processes);
            prop_assert!(counts(&next).iter().all(|n| *n >= s.min_processes));
        }

        /// From a valid allocation, each queue moves by at most the shift
        /// (unless it was below its minimum) and reaches the target.
        #[test]
        fn auto_moves_steadily_to_its_target((s, _current, work, shift) in arbitrary_case()) {
            let target = target_allocation(&s, &work);
            let mut current = initial_allocation(&s);
            for _ in 0..(s.max_processes + 2) {
                let next = next_allocation(&s, &current, &work, shift);
                for ((_, before), (_, after)) in current.iter().zip(&next) {
                    prop_assert!(before.abs_diff(*after) <= shift);
                }
                current = next;
            }
            prop_assert_eq!(current, target);
        }
    }
}
