//! The C interface: five functions that take and return JSON as bytes. This
//! is the only file of the crate that holds `unsafe`.
//!
//! The header is `include/navree_core.h`. A client makes one core with
//! [`navree_core_new`], tells it what happened with [`navree_core_send`],
//! reads the parts that changed with [`navree_core_view`], gives every answer
//! back with [`navree_bytes_free`], and ends with [`navree_core_free`].
//!
//! What a client must guarantee:
//! - Events are sent in the order things happened. The simplest way is to
//!   call from one thread, and that is what the clients do. A lock inside the
//!   handle keeps two calls made at once from running together, so a mistake
//!   here costs order, never memory.
//! - Bytes passed in stay valid for the call. The core copies what it keeps.
//! - Every [`NavreeBytes`] that comes back is given to [`navree_bytes_free`]
//!   exactly once.
//!
//! What a client may assume of the handle: freeing it while a call is in
//! progress on another thread waits for that call to finish, and a call that
//! comes during or after the free answers `{"error":"..."}`. The core inside
//! is dropped at the free; the handle's own few bytes (a lock and a mark) are
//! never given back to the allocator, so no call can read memory that was
//! freed. That costs a few bytes for every core made in a process, and there
//! is one core for each run of an app.
//!
//! No call panics across the boundary. A panic inside the core is caught, the
//! call answers `{"error":"..."}`, and the core stops for good: what it held
//! is dropped, nothing it had queued is sent, and every later call on the
//! handle answers the same error. A client shows that the app must be started
//! again.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use crate::{Config, Core, Event, Query};

/// A core behind the C interface. The handle itself lives as long as the
/// process; [`navree_core_free`] drops the core inside it.
pub struct NavreeCore {
    core: Mutex<Slot>,
}

/// What a handle holds.
enum Slot {
    /// A core in use.
    Live(Box<Core>),
    /// A core that failed inside a call. Its state, and every effect it had
    /// queued, were thrown away; every later call answers [`STOPPED`].
    Stopped,
    /// A handle given to [`navree_core_free`]. Every later call answers
    /// [`FREED`].
    Freed,
}

/// What every call answers once the core has stopped.
const STOPPED: &str = "the core stopped after a failure inside it and can't be used again";

/// What every call answers once the handle was freed.
const FREED: &str = "the core was freed";

/// Bytes the core owns: UTF-8 JSON, `len` bytes at `ptr`, not ended by a
/// zero. An empty answer has a null `ptr` and a `len` of zero.
#[repr(C)]
pub struct NavreeBytes {
    /// Where the bytes are.
    pub ptr: *mut u8,
    /// How many there are.
    pub len: usize,
}

fn give(bytes: Vec<u8>) -> NavreeBytes {
    if bytes.is_empty() {
        return NavreeBytes {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
    }
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    NavreeBytes {
        ptr: Box::into_raw(boxed).cast::<u8>(),
        len,
    }
}

fn error(what: &str) -> Vec<u8> {
    serde_json::json!({ "error": what })
        .to_string()
        .into_bytes()
}

/// The bytes a caller passed, or nothing for a null pointer.
///
/// # Safety
/// `ptr` is null, or points to `len` bytes that stay valid for the call.
unsafe fn taken<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller says `ptr` points to `len` readable bytes.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

fn new(config: &[u8]) -> Option<Box<NavreeCore>> {
    let config: Config = serde_json::from_slice(config).ok()?;
    let core = catch_unwind(|| Box::new(Core::new(config))).ok()?;
    Some(Box::new(NavreeCore {
        core: Mutex::new(Slot::Live(core)),
    }))
}

/// Runs `call` on the handle's core, one call at a time. A failure inside
/// it, or an answer that does not write, stops the core for good: what it
/// held is dropped, so nothing it had queued is sent with a later answer,
/// and this call and every later one answer an error.
fn using(
    handle: &NavreeCore,
    call: impl FnOnce(&mut Core) -> serde_json::Result<Vec<u8>>,
) -> Vec<u8> {
    // A lock poisoned by a failure that escaped is a stopped core too.
    let mut slot = handle
        .core
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let core = match &mut *slot {
        Slot::Live(core) => core,
        Slot::Stopped => return error(STOPPED),
        Slot::Freed => return error(FREED),
    };
    match catch_unwind(AssertUnwindSafe(|| call(core))) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) | Err(_) => {
            let failed = std::mem::replace(&mut *slot, Slot::Stopped);
            // Dropping plain data does not fail; if it ever did, the failure
            // still stays on this side, and the slot already says stopped.
            let _ = catch_unwind(AssertUnwindSafe(|| drop(failed)));
            error(STOPPED)
        }
    }
}

fn send(handle: &NavreeCore, event: &[u8]) -> Vec<u8> {
    let event: Event = match serde_json::from_slice(event) {
        Ok(event) => event,
        Err(problem) => return error(&format!("the event does not read: {problem}")),
    };
    using(handle, |core| {
        #[cfg(test)]
        tests::before_send(core);
        serde_json::to_vec(&core.send(event))
    })
}

fn view(handle: &NavreeCore, query: &[u8]) -> Vec<u8> {
    let query: Query = if query.is_empty() {
        Query::All
    } else {
        match serde_json::from_slice(query) {
            Ok(query) => query,
            Err(problem) => return error(&format!("the query does not read: {problem}")),
        }
    };
    using(handle, |core| serde_json::to_vec(&core.part(&query)))
}

/// Makes a core from a configuration, as JSON. Null when the configuration
/// does not read.
///
/// # Safety
/// `config` is null, or points to `len` bytes that stay valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_new(config: *const u8, len: usize) -> *mut NavreeCore {
    // SAFETY: the caller upholds what `taken` asks.
    let config = unsafe { taken(config, len) };
    new(config).map_or(std::ptr::null_mut(), Box::into_raw)
}

/// Tells the core what happened: an event, as JSON. Answers the update, as
/// JSON, or `{"error":"..."}`.
///
/// # Safety
/// `core` is null or came from [`navree_core_new`] (freed or not). `event` is null, or
/// points to `len` bytes that stay valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_send(
    core: *mut NavreeCore,
    event: *const u8,
    len: usize,
) -> NavreeBytes {
    // SAFETY: the caller says `core` came from `navree_core_new`; a handle is
    // never given back to the allocator, so it is there even after a free. It
    // is only read here, and the lock inside it is what gives the core to one
    // call at a time.
    let Some(handle) = (unsafe { core.as_ref() }) else {
        return give(error("no core"));
    };
    // SAFETY: the caller upholds what `taken` asks.
    give(send(handle, unsafe { taken(event, len) }))
}

/// Reads a part of the view state: a query, as JSON (`{"part":"nav"}`), or
/// nothing for all of it. Answers the part, as JSON, or `{"error":"..."}`.
///
/// # Safety
/// `core` is null or came from [`navree_core_new`] (freed or not). `query` is null, or
/// points to `len` bytes that stay valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_view(
    core: *const NavreeCore,
    query: *const u8,
    len: usize,
) -> NavreeBytes {
    // SAFETY: as in `navree_core_send`.
    let Some(handle) = (unsafe { core.as_ref() }) else {
        return give(error("no core"));
    };
    // SAFETY: the caller upholds what `taken` asks.
    give(view(handle, unsafe { taken(query, len) }))
}

/// Gives bytes back to the core.
///
/// # Safety
/// `bytes` came from a function of this interface and was not given back before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_bytes_free(bytes: NavreeBytes) {
    if !bytes.ptr.is_null() {
        // SAFETY: `give` made these from a boxed slice of exactly this length,
        // and the caller says they have not been freed.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes.ptr, bytes.len)) });
    }
}

/// Ends a core. A call in progress on another thread finishes first; a call
/// that comes later, and a second free, answer an error or do nothing.
///
/// # Safety
/// `core` is null, or came from [`navree_core_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_free(core: *mut NavreeCore) {
    // SAFETY: the caller says `core` came from `navree_core_new`, and the
    // handle is never given back to the allocator, so it is still there
    // however many times it was freed. It is only read here; the lock inside
    // it waits for a call in progress.
    let Some(handle) = (unsafe { core.as_ref() }) else {
        return;
    };
    let mut slot = handle
        .core
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let held = std::mem::replace(&mut *slot, Slot::Freed);
    drop(slot);
    // As in `using`: a failure while dropping stays on this side.
    let _ = catch_unwind(AssertUnwindSafe(|| drop(held)));
}

/// The glue for the Kotlin class `com.navree.core.Core`, whose four static
/// native methods take and return `ByteArray`s of JSON and keep the handle as
/// a `Long`.
#[cfg(feature = "jni")]
pub mod jni {
    use jni_sys::{JNIEnv, jbyte, jbyteArray, jclass, jlong, jsize};

    use super::{NavreeCore, error, new, send, view};

    /// Copies a Java byte array.
    ///
    /// # Safety
    /// `env` is the environment of the calling thread and `array` a live
    /// reference to a byte array, or null.
    unsafe fn read(env: *mut JNIEnv, array: jbyteArray) -> Vec<u8> {
        if array.is_null() {
            return Vec::new();
        }
        // SAFETY: the function table of a live environment is valid, and
        // `array` is a byte array, as the method's signature says.
        unsafe {
            let table = (**env).v1_1;
            let len = (table.GetArrayLength)(env, array);
            let mut bytes = vec![0u8; usize::try_from(len).unwrap_or(0)];
            (table.GetByteArrayRegion)(env, array, 0, len, bytes.as_mut_ptr().cast::<jbyte>());
            bytes
        }
    }

    /// Makes a Java byte array of `bytes`. Null when Java could not make it,
    /// with Java's own error left pending.
    ///
    /// # Safety
    /// `env` is the environment of the calling thread.
    unsafe fn write(env: *mut JNIEnv, bytes: &[u8]) -> jbyteArray {
        let Ok(len) = jsize::try_from(bytes.len()) else {
            return std::ptr::null_mut();
        };
        // SAFETY: the function table of a live environment is valid; the
        // region written is inside the array just made.
        unsafe {
            let table = (**env).v1_1;
            let array = (table.NewByteArray)(env, len);
            if !array.is_null() {
                (table.SetByteArrayRegion)(env, array, 0, len, bytes.as_ptr().cast::<jbyte>());
            }
            array
        }
    }

    /// `private external fun nativeNew(config: ByteArray): Long`. Zero when
    /// the configuration does not read.
    ///
    /// # Safety
    /// Called by the Java runtime with a live environment.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeNew(
        env: *mut JNIEnv,
        _class: jclass,
        config: jbyteArray,
    ) -> jlong {
        // SAFETY: the runtime passes a live environment and a byte array.
        let config = unsafe { read(env, config) };
        new(&config).map_or(0, |core| Box::into_raw(core) as jlong)
    }

    /// `private external fun nativeSend(handle: Long, event: ByteArray): ByteArray`.
    ///
    /// # Safety
    /// Called by the Java runtime with a live environment; `handle` came from
    /// `nativeNew` (freed or not).
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeSend(
        env: *mut JNIEnv,
        _class: jclass,
        handle: jlong,
        event: jbyteArray,
    ) -> jbyteArray {
        // SAFETY: the runtime passes a live environment; the handle came from
        // `nativeNew`, and a handle is never given back to the allocator.
        unsafe {
            let event = read(env, event);
            let out = match (handle as *const NavreeCore).as_ref() {
                Some(handle) => send(handle, &event),
                None => error("no core"),
            };
            write(env, &out)
        }
    }

    /// `private external fun nativeView(handle: Long, query: ByteArray): ByteArray`.
    ///
    /// # Safety
    /// As `nativeSend`.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeView(
        env: *mut JNIEnv,
        _class: jclass,
        handle: jlong,
        query: jbyteArray,
    ) -> jbyteArray {
        // SAFETY: as in `nativeSend`.
        unsafe {
            let query = read(env, query);
            let out = match (handle as *const NavreeCore).as_ref() {
                Some(handle) => view(handle, &query),
                None => error("no core"),
            };
            write(env, &out)
        }
    }

    /// `private external fun nativeFree(handle: Long)`.
    ///
    /// # Safety
    /// `handle` is zero, or came from `nativeNew`.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeFree(
        _env: *mut JNIEnv,
        _class: jclass,
        handle: jlong,
    ) {
        // SAFETY: the caller upholds what `navree_core_free` asks.
        unsafe { super::navree_core_free(handle as *mut NavreeCore) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    type Hook = Box<dyn FnOnce(&mut Core)>;

    thread_local! {
        /// What the next `send` on this thread does to the core before the
        /// event, so a test can make the core fail halfway through a call.
        static BEFORE_SEND: RefCell<Option<Hook>> = RefCell::new(None);
    }

    pub(super) fn before_send(core: &mut Core) {
        if let Some(hook) = BEFORE_SEND.with(|hook| hook.borrow_mut().take()) {
            hook(core);
        }
    }

    fn on_next_send(hook: impl FnOnce(&mut Core) + 'static) {
        BEFORE_SEND.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    fn text(bytes: NavreeBytes) -> String {
        // SAFETY: `bytes` came from this interface just now and is freed once, here.
        unsafe {
            let out = String::from_utf8(taken(bytes.ptr, bytes.len).to_vec()).unwrap();
            navree_bytes_free(bytes);
            out
        }
    }

    const CONFIG: &str = r#"{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":1,"text_percent":100}}"#;

    #[test]
    fn the_c_interface_round_trips() {
        // SAFETY: every pointer passed points to a live string of the length given;
        // the handle is used until it is freed and not after.
        unsafe {
            let core = navree_core_new(CONFIG.as_ptr(), CONFIG.len());
            assert!(!core.is_null());
            let event = br#"{"t":"start"}"#;
            let update = text(navree_core_send(core, event.as_ptr(), event.len()));
            let update: serde_json::Value = serde_json::from_str(&update).unwrap();
            assert_eq!(update["effects"][0]["t"], "fetch");
            assert_eq!(
                update["effects"][0]["url"],
                "https://shop.example/_navree/native/shell"
            );

            let all = text(navree_core_view(core, std::ptr::null(), 0));
            let all: serde_json::Value = serde_json::from_str(&all).unwrap();
            assert_eq!(all["stage"], "starting");
            let query = br#"{"part":"nav"}"#;
            let nav = text(navree_core_view(core, query.as_ptr(), query.len()));
            assert!(nav.contains(r#""stage":"starting""#), "{nav}");
            navree_core_free(core);
        }
    }

    #[test]
    fn a_core_that_failed_inside_stays_stopped_and_sends_nothing_it_had_queued() {
        // SAFETY: as above; the handle is used until it is freed and not after.
        unsafe {
            let core = navree_core_new(CONFIG.as_ptr(), CONFIG.len());
            let start = br#"{"t":"start"}"#;
            assert!(text(navree_core_send(core, start.as_ptr(), start.len())).contains("fetch"));

            // The core queues something to send and then fails before the
            // call ends.
            on_next_send(|core| {
                core.effects.push(crate::Effect::CacheClear);
                panic!("a failure inside the core");
            });
            let back = br#"{"t":"back"}"#;
            let failed = text(navree_core_send(core, back.as_ptr(), back.len()));
            assert!(failed.contains(r#""error""#), "{failed}");

            // Every later call answers that the core is stopped, and what it
            // had queued is never sent.
            for _ in 0..2 {
                let later = text(navree_core_send(core, back.as_ptr(), back.len()));
                assert!(later.contains("stopped"), "{later}");
                assert!(!later.contains("cache_clear"), "{later}");
            }
            let all = text(navree_core_view(core, std::ptr::null(), 0));
            assert!(all.contains("stopped"), "{all}");
            let nav = br#"{"part":"nav"}"#;
            assert!(text(navree_core_view(core, nav.as_ptr(), nav.len())).contains("stopped"));
            navree_core_free(core);
        }
    }

    #[test]
    fn freeing_a_core_while_another_thread_is_in_a_call_waits_for_the_call_and_refuses_later_ones()
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, mpsc};
        use std::time::Duration;

        // SAFETY: as above.
        let core = unsafe { navree_core_new(CONFIG.as_ptr(), CONFIG.len()) } as usize;
        let (inside, is_inside) = mpsc::channel();
        let (go, wait) = mpsc::channel::<()>();
        let call = std::thread::spawn(move || {
            on_next_send(move |_| {
                inside.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(10)).unwrap();
            });
            let start = br#"{"t":"start"}"#;
            // SAFETY: the handle is live when the call starts; freeing it is
            // what is being tested.
            text(unsafe { navree_core_send(core as *mut NavreeCore, start.as_ptr(), start.len()) })
        });
        is_inside.recv_timeout(Duration::from_secs(10)).unwrap();

        let freed = Arc::new(AtomicBool::new(false));
        let free = std::thread::spawn({
            let freed = Arc::clone(&freed);
            move || {
                // SAFETY: freed once, while the other thread's call is in progress.
                unsafe { navree_core_free(core as *mut NavreeCore) };
                freed.store(true, Ordering::SeqCst);
            }
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !freed.load(Ordering::SeqCst),
            "the free did not wait for the call in progress"
        );
        go.send(()).unwrap();

        // The call in progress finishes with its answer.
        let answer = call.join().unwrap();
        assert!(answer.contains(r#""t":"fetch""#), "{answer}");
        free.join().unwrap();
        assert!(freed.load(Ordering::SeqCst));

        // A call that comes after the free is refused, and a second free does
        // nothing.
        // SAFETY: what is being tested: the interface answers these itself.
        unsafe {
            let start = br#"{"t":"start"}"#;
            let late = text(navree_core_send(
                core as *mut NavreeCore,
                start.as_ptr(),
                start.len(),
            ));
            assert!(late.contains("freed"), "{late}");
            assert!(
                text(navree_core_view(
                    core as *const NavreeCore,
                    std::ptr::null(),
                    0
                ))
                .contains("freed")
            );
            navree_core_free(core as *mut NavreeCore);
        }
    }

    #[test]
    fn a_bad_argument_is_an_error_not_a_crash() {
        // SAFETY: as above; null pointers are what is being tested.
        unsafe {
            assert!(navree_core_new(std::ptr::null(), 0).is_null());
            let junk = b"{not json";
            assert!(navree_core_new(junk.as_ptr(), junk.len()).is_null());
            assert!(
                text(navree_core_send(std::ptr::null_mut(), std::ptr::null(), 0))
                    .contains("no core")
            );

            let core = navree_core_new(CONFIG.as_ptr(), CONFIG.len());
            let unknown = br#"{"t":"teleport"}"#;
            assert!(
                text(navree_core_send(core, unknown.as_ptr(), unknown.len())).contains("error")
            );
            assert!(text(navree_core_send(core, std::ptr::null(), 0)).contains("error"));
            let query = br#"{"part":"nowhere"}"#;
            assert!(text(navree_core_view(core, query.as_ptr(), query.len())).contains("error"));
            let gone = br#"{"part":"screen","id":99}"#;
            assert_eq!(
                text(navree_core_view(core, gone.as_ptr(), gone.len())),
                "null"
            );
            navree_bytes_free(NavreeBytes {
                ptr: std::ptr::null_mut(),
                len: 0,
            });
            navree_core_free(std::ptr::null_mut());
            navree_core_free(core);
        }
    }
}
