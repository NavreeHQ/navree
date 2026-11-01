//! The stack a thread's compiles and renders may use.
//!
//! There is one budget per thread: the outermost compile or render on a thread
//! measures from where it starts and from what the thread has left, and
//! whatever runs inside it (a render a function starts, a template a loader
//! compiles) counts against the same start. A value walk does not check the
//! stack at each level, so a render stops while [`RESERVE`] is still free.

use std::cell::Cell;
use std::marker::PhantomData;

/// What stays free for what does not check the stack: printing, comparing and
/// dropping a value nested as deep as values may, in any build.
const RESERVE: usize = 256 << 10;

/// The outermost compile or render running on a thread.
#[derive(Clone, Copy)]
struct Start {
    /// Where it started.
    address: usize,
    /// How far the stack may grow from there and keep [`RESERVE`] free.
    room: usize,
    /// The compiles and renders running.
    entries: usize,
}

thread_local! {
    static START: Cell<Start> = const {
        Cell::new(Start { address: 0, room: usize::MAX, entries: 0 })
    };
}

/// The stack a compile or render may use. Leaving it, by returning or by a
/// panic, gives the entry back; the thread's start ends with the outermost.
pub(crate) struct StackBudget {
    start: usize,
    allowed: usize,
    /// Tied to the thread whose stack it measures.
    thread: PhantomData<*const ()>,
}

impl StackBudget {
    /// Enters a compile or render that may use `max_bytes` from where the
    /// thread's outermost one started.
    pub(crate) fn enter(max_bytes: usize) -> StackBudget {
        let start = START.with(|start| {
            let mut current = start.get();
            if current.entries == 0 {
                current.address = stack_address();
                // Without a known end of the stack, only `max_bytes` applies.
                current.room = stacker::remaining_stack()
                    .map_or(usize::MAX, |left| left.saturating_sub(RESERVE));
            }
            current.entries += 1;
            start.set(current);
            current
        });
        StackBudget {
            start: start.address,
            allowed: max_bytes.min(start.room),
            thread: PhantomData,
        }
    }

    /// Whether the budget is used up.
    #[inline]
    pub(crate) fn exceeded(&self) -> bool {
        stack_address().abs_diff(self.start) > self.allowed
    }
}

impl Drop for StackBudget {
    fn drop(&mut self) {
        START.with(|start| {
            let mut current = start.get();
            current.entries = current.entries.saturating_sub(1);
            start.set(current);
        });
    }
}

/// Where the stack is now: comparing two of these tells how much is used
/// between them.
///
/// Fuzz builds run under AddressSanitizer, where locals do not live on the
/// stack, so there `Limits::max_stack_bytes` cannot measure it and does not
/// apply.
#[inline(always)]
#[allow(unexpected_cfgs)]
fn stack_address() -> usize {
    if cfg!(fuzzing) {
        return 0;
    }
    let marker = 0_u8;
    std::ptr::addr_of!(marker) as usize
}
