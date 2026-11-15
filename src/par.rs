//! Deterministic ordered parallel map over indexed work items (std only).
//!
//! Workers claim indices from one atomic counter and store a per-index result.
//! The caller receives results in index order, or the error of the lowest
//! failing index, exactly like a sequential `for` loop with `?`. The claim
//! order may differ from the index order (large files first), but the result
//! order and the reported error never do.

use crate::error::Result;
use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Condvar, Mutex, MutexGuard,
    },
    thread,
};

/// Smallest bytes-in-flight allowance: typical repositories run at full width.
pub(crate) const MIN_BYTES_IN_FLIGHT: usize = 1 << 20;

/// Worker cap for `jobs`: the requested count, or the available parallelism.
pub(crate) fn resolve(jobs: Option<NonZeroUsize>) -> usize {
    jobs.or_else(|| thread::available_parallelism().ok())
        .map_or(1, NonZeroUsize::get)
}

/// Worker count for `items` units of work under `cap`: never above `items`, never zero.
pub(crate) fn workers_for(items: usize, cap: usize) -> usize {
    cap.min(items).max(1)
}

/// Claim order: highest `weight` first, ties in index order, so one large item
/// does not become the tail of the run.
fn schedule(items: usize, weight: impl Fn(usize) -> u64) -> Vec<usize> {
    let mut order: Vec<(u64, usize)> = (0..items).map(|i| (weight(i), i)).collect();
    order.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    order.into_iter().map(|(_, i)| i).collect()
}

/// Run `work(state, index)` for every index in `0..items` using one state per worker.
///
/// `states` fixes the worker count (one thread per state; a single state runs
/// inline on the calling thread, in index order, without spawning and without
/// calling `weight`). With several workers, indices are claimed in descending
/// `weight` order. Returns the results in index order, or the error with the
/// lowest index. Indices above the lowest known failure are skipped, so work
/// past the first error is abandoned, but every index below it is completed.
pub(crate) fn ordered_map<S: Send, T: Send>(
    items: usize,
    mut states: Vec<S>,
    weight: impl Fn(usize) -> u64,
    work: impl Fn(&mut S, usize) -> Result<T> + Sync,
) -> Result<Vec<T>> {
    if states.len() <= 1 {
        let mut state = states.pop();
        return (0..items)
            .map(|i| work(state.as_mut().expect("one state"), i))
            .collect();
    }
    let order = schedule(items, weight);
    let next = AtomicUsize::new(0);
    let first_failure = AtomicUsize::new(usize::MAX);
    let slots: Vec<Mutex<Option<Result<T>>>> = (0..items).map(|_| Mutex::new(None)).collect();
    thread::scope(|scope| {
        let handles: Vec<_> = states
            .iter_mut()
            .map(|state| {
                let (order, next, first_failure, slots, work) =
                    (&order, &next, &first_failure, &slots, &work);
                scope.spawn(move || {
                    while let Some(&i) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
                        if i > first_failure.load(Ordering::Relaxed) {
                            continue;
                        }
                        let result = work(state, i);
                        if result.is_err() {
                            first_failure.fetch_min(i, Ordering::Relaxed);
                        }
                        *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
                    }
                    handoff()
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(counts) => absorb(counts),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .expect("every index up to the first failure is completed")
        })
        .collect()
}

/// A worker's test-only work counts, handed to the calling thread on exit.
#[cfg(test)]
fn handoff() -> [u64; 4] {
    crate::work::take()
}
#[cfg(not(test))]
fn handoff() {}
/// Add a finished worker's counts to the calling thread's counts.
#[cfg(test)]
fn absorb(counts: [u64; 4]) {
    crate::work::add(counts);
}
#[cfg(not(test))]
fn absorb((): ()) {}

/// Bound on the source bytes being parsed at once.
///
/// A parse of `n` bytes may start only while the bytes already being parsed
/// plus `n` stay within `max(floor, n)`. A parse always starts when nothing else
/// is in flight, so a file larger than the floor runs alone and a repository of
/// very large files degrades to sequential parsing with today's peak.
pub(crate) struct ByteBudget {
    floor: usize,
    state: Mutex<BudgetState>,
    released: Condvar,
}

/// Bytes currently admitted and the most ever admitted at once.
#[derive(Default)]
struct BudgetState {
    in_flight: usize,
    peak: usize,
}

impl ByteBudget {
    /// A budget whose allowance is never below `floor` bytes.
    pub(crate) fn new(floor: usize) -> Self {
        Self {
            floor,
            state: Mutex::default(),
            released: Condvar::new(),
        }
    }
    /// Block until `bytes` may be parsed; the permit releases them when dropped.
    pub(crate) fn acquire(&self, bytes: usize) -> Permit<'_> {
        let limit = self.floor.max(bytes);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // `bytes <= limit`, so this never waits when nothing is in flight.
        while state.in_flight.saturating_add(bytes) > limit {
            state = self.wait(state);
        }
        state.in_flight += bytes;
        state.peak = state.peak.max(state.in_flight);
        Permit {
            budget: self,
            bytes,
        }
    }
    /// Wait for a release.
    #[cfg(not(test))]
    fn wait<'a>(&self, state: MutexGuard<'a, BudgetState>) -> MutexGuard<'a, BudgetState> {
        self.released.wait(state).unwrap_or_else(|e| e.into_inner())
    }
    /// Wait for a release; a wait that never ends fails the test instead of hanging it.
    #[cfg(test)]
    fn wait<'a>(&self, state: MutexGuard<'a, BudgetState>) -> MutexGuard<'a, BudgetState> {
        let (state, wait) = self
            .released
            .wait_timeout(state, std::time::Duration::from_secs(30))
            .unwrap_or_else(|e| e.into_inner());
        assert!(!wait.timed_out(), "byte budget wait did not end");
        state
    }
    /// The most bytes ever admitted at once.
    #[cfg(test)]
    pub(crate) fn peak(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).peak
    }
}

/// Admission of some bytes to a [`ByteBudget`], released on drop.
pub(crate) struct Permit<'a> {
    budget: &'a ByteBudget,
    bytes: usize,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap_or_else(|e| e.into_inner());
        state.in_flight -= self.bytes;
        drop(state);
        self.budget.released.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{fail, ErrorKind};
    use std::time::{Duration, Instant};

    #[test]
    fn jobs_resolve_to_the_request_or_the_available_parallelism() {
        assert_eq!(resolve(NonZeroUsize::new(3)), 3);
        assert_eq!(resolve(NonZeroUsize::new(1)), 1);
        let available = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        assert_eq!(resolve(None), available);
    }

    #[test]
    fn workers_are_capped_by_items_and_never_zero() {
        assert_eq!(workers_for(0, 8), 1);
        assert_eq!(workers_for(1, 8), 1);
        assert_eq!(workers_for(3, 8), 3);
        assert_eq!(workers_for(100, 8), 8);
        assert_eq!(workers_for(100, 0), 1);
    }

    #[test]
    fn schedule_claims_heaviest_first_and_breaks_ties_by_index() {
        let weights = [5, 9, 1, 9, 0, 5];
        assert_eq!(schedule(6, |i| weights[i]), [1, 3, 0, 5, 2, 4]);
        assert!(schedule(0, |_| unreachable!()).is_empty());
    }

    #[test]
    fn results_keep_index_order_for_any_worker_count() {
        for workers in 1..=8 {
            let out =
                ordered_map(50, vec![(); workers], |i| (i % 7) as u64, |(), i| Ok(i * 2)).unwrap();
            assert_eq!(out, (0..50).map(|i| i * 2).collect::<Vec<_>>());
        }
        assert!(ordered_map(0, vec![(); 4], |_| 0, |(), i| Ok(i))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn every_worker_state_is_used() {
        // Each item waits for the other to start, so two workers must hold one each.
        // The wait is bounded so a broken pool fails instead of hanging.
        let arrived = AtomicUsize::new(0);
        let claimed = Mutex::new(Vec::new());
        let used = ordered_map(
            2,
            vec![0usize; 2],
            |i| i as u64,
            |state, i| {
                claimed.lock().unwrap().push(i);
                arrived.fetch_add(1, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(20);
                while arrived.load(Ordering::SeqCst) < 2 {
                    assert!(
                        Instant::now() < deadline,
                        "workers did not run concurrently"
                    );
                    thread::yield_now();
                }
                *state += 1;
                Ok(*state)
            },
        )
        .unwrap();
        assert_eq!(used, [1, 1]);
        assert_eq!(claimed.into_inner().unwrap().len(), 2);
    }

    #[test]
    fn lowest_failing_index_wins_even_when_a_later_one_fails_first() {
        for _ in 0..40 {
            let error = ordered_map(
                8,
                vec![(); 4],
                |_| 0,
                |(), i| match i {
                    // The earliest failure is slow; later failures finish immediately.
                    2 => {
                        thread::sleep(Duration::from_millis(15));
                        fail!(Syntax, "failure 2")
                    }
                    5 | 7 => fail!(Io, "failure later"),
                    _ => Ok(i),
                },
            )
            .unwrap_err();
            assert_eq!(error.to_string(), "failure 2");
            assert_eq!(error.kind(), ErrorKind::Syntax);
        }
    }

    #[test]
    fn a_later_failure_claimed_first_does_not_hide_an_earlier_one() {
        // Index 3 is heaviest, so it is claimed and fails first; 0..3 must still run.
        let ran = Mutex::new(Vec::new());
        let error = ordered_map(
            6,
            vec![(); 2],
            |i| if i == 3 { 100 } else { 0 },
            |(), i| {
                ran.lock().unwrap().push(i);
                match i {
                    1 => fail!(Coverage, "first"),
                    3 => fail!(Io, "claimed first"),
                    _ => Ok(i),
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "first");
        assert_eq!(error.kind(), ErrorKind::Coverage);
        let ran = ran.into_inner().unwrap();
        assert!(ran.contains(&3));
        assert!(ran.contains(&0) && ran.contains(&1));
    }

    #[test]
    fn work_after_the_first_failure_is_abandoned() {
        let calls = AtomicUsize::new(0);
        let error = ordered_map(
            10_000,
            vec![(); 2],
            |_| 0,
            |(), i| {
                calls.fetch_add(1, Ordering::Relaxed);
                if i == 0 {
                    fail!(Io, "boom");
                }
                thread::sleep(Duration::from_micros(200));
                Ok(i)
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "boom");
        assert!(calls.load(Ordering::Relaxed) < 10_000);
    }

    #[test]
    fn a_single_state_runs_inline_in_index_order_and_stops_at_the_first_failure() {
        let main = thread::current().id();
        let seen = Mutex::new(Vec::new());
        let error = ordered_map(
            5,
            vec![()],
            |_| unreachable!("a single worker never schedules"),
            |(), i| {
                assert_eq!(thread::current().id(), main);
                seen.lock().unwrap().push(i);
                if i == 2 {
                    fail!(Io, "stop");
                }
                Ok(i)
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "stop");
        assert_eq!(seen.into_inner().unwrap(), [0, 1, 2]);
    }

    #[test]
    fn workers_hand_their_work_counts_to_the_caller() {
        crate::work::take();
        ordered_map(
            6,
            vec![(); 3],
            |_| 0,
            |(), _| {
                crate::work::count(crate::work::Unit::Parse);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(crate::work::take(), [0, 0, 0, 6]);
    }

    #[test]
    fn worker_panics_propagate_to_the_caller() {
        let panicked = std::panic::catch_unwind(|| {
            ordered_map(
                4,
                vec![(); 2],
                |_| 0,
                |(), i| {
                    assert_ne!(i, 2, "worker panic");
                    Ok(i)
                },
            )
        });
        assert!(panicked.is_err());
    }

    #[test]
    fn budget_admits_within_the_floor_and_a_large_item_alone() {
        let budget = ByteBudget::new(100);
        let a = budget.acquire(40);
        let b = budget.acquire(60);
        assert_eq!(budget.peak(), 100);
        drop((a, b));
        assert_eq!(budget.state.lock().unwrap().in_flight, 0);
        let big = budget.acquire(500);
        assert_eq!(budget.peak(), 500);
        drop(big);
        let empty = budget.acquire(0);
        drop(empty);
        assert_eq!(budget.state.lock().unwrap().in_flight, 0);
    }

    #[test]
    fn budget_blocks_until_bytes_are_released() {
        let budget = ByteBudget::new(100);
        let held = budget.acquire(80);
        let admitted = AtomicUsize::new(0);
        thread::scope(|scope| {
            scope.spawn(|| {
                let _permit = budget.acquire(30);
                admitted.store(1, Ordering::SeqCst);
            });
            thread::sleep(Duration::from_millis(50));
            assert_eq!(admitted.load(Ordering::SeqCst), 0, "admitted over budget");
            drop(held);
        });
        assert_eq!(admitted.load(Ordering::SeqCst), 1);
        assert_eq!(budget.peak(), 80);
    }

    #[test]
    fn budget_bounds_bytes_in_flight_under_contention() {
        let budget = ByteBudget::new(1000);
        let sizes = [900, 700, 1500, 300, 1200, 50, 999, 2000, 10, 600];
        let live = AtomicUsize::new(0);
        let worst = AtomicUsize::new(0);
        ordered_map(
            sizes.len(),
            vec![(); 6],
            |i| sizes[i],
            |(), i| {
                let size = sizes[i] as usize;
                let _permit = budget.acquire(size);
                let now = live.fetch_add(size, Ordering::SeqCst) + size;
                worst.fetch_max(now, Ordering::SeqCst);
                assert!(now <= 1000.max(size), "{now} bytes in flight with {size}");
                thread::sleep(Duration::from_millis(5));
                live.fetch_sub(size, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert!(budget.peak() <= 2000);
        assert_eq!(live.load(Ordering::SeqCst), 0);
        assert!(worst.load(Ordering::SeqCst) >= 2000);
    }
}
