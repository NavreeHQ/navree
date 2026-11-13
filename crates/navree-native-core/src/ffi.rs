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
//!   exactly once, and a handle is not used after [`navree_core_free`].
//!
//! No call panics across the boundary: a panic inside the core is caught and
//! the call answers `{"error":"..."}`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use crate::{Config, Core, Event, Query};

/// A core behind the C interface.
pub struct NavreeCore {
    core: Mutex<Core>,
}

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
    serde_json::json!({ "error": what }).to_string().into_bytes()
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
    let core = catch_unwind(|| Core::new(config)).ok()?;
    Some(Box::new(NavreeCore { core: Mutex::new(core) }))
}

fn send(handle: &NavreeCore, event: &[u8]) -> Vec<u8> {
    let event: Event = match serde_json::from_slice(event) {
        Ok(event) => event,
        Err(problem) => return error(&format!("the event does not read: {problem}")),
    };
    let Ok(mut core) = handle.core.lock() else {
        return error("the core stopped after an earlier failure");
    };
    match catch_unwind(AssertUnwindSafe(|| core.send(event))) {
        Ok(update) => serde_json::to_vec(&update).unwrap_or_else(|_| error("the update does not write")),
        Err(_) => error("the core failed on this event"),
    }
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
    let Ok(core) = handle.core.lock() else {
        return error("the core stopped after an earlier failure");
    };
    match catch_unwind(AssertUnwindSafe(|| core.part(&query))) {
        Ok(part) => serde_json::to_vec(&part).unwrap_or_else(|_| error("the view does not write")),
        Err(_) => error("the core failed on this query"),
    }
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
/// `core` came from [`navree_core_new`] and was not freed. `event` is null, or
/// points to `len` bytes that stay valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_send(core: *mut NavreeCore, event: *const u8, len: usize) -> NavreeBytes {
    // SAFETY: the caller says `core` is a live handle; it is only read here,
    // and the lock inside it is what gives the core to one call at a time.
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
/// `core` came from [`navree_core_new`] and was not freed. `query` is null, or
/// points to `len` bytes that stay valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_view(core: *const NavreeCore, query: *const u8, len: usize) -> NavreeBytes {
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

/// Ends a core.
///
/// # Safety
/// `core` is null, or came from [`navree_core_new`] and was not freed, and no
/// other call is using it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn navree_core_free(core: *mut NavreeCore) {
    if !core.is_null() {
        // SAFETY: `navree_core_new` made this with `Box::into_raw`, and the
        // caller says it has not been freed and is not in use.
        drop(unsafe { Box::from_raw(core) });
    }
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
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeNew(env: *mut JNIEnv, _class: jclass, config: jbyteArray) -> jlong {
        // SAFETY: the runtime passes a live environment and a byte array.
        let config = unsafe { read(env, config) };
        new(&config).map_or(0, |core| Box::into_raw(core) as jlong)
    }

    /// `private external fun nativeSend(handle: Long, event: ByteArray): ByteArray`.
    ///
    /// # Safety
    /// Called by the Java runtime with a live environment; `handle` came from
    /// `nativeNew` and was not freed.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeSend(env: *mut JNIEnv, _class: jclass, handle: jlong, event: jbyteArray) -> jbyteArray {
        // SAFETY: the runtime passes a live environment; the class keeps the
        // handle alive until `nativeFree`.
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
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeView(env: *mut JNIEnv, _class: jclass, handle: jlong, query: jbyteArray) -> jbyteArray {
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
    /// `handle` is zero, or came from `nativeNew` and was not freed, and no
    /// other call is using it.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_navree_core_Core_nativeFree(_env: *mut JNIEnv, _class: jclass, handle: jlong) {
        // SAFETY: the caller upholds what `navree_core_free` asks.
        unsafe { super::navree_core_free(handle as *mut NavreeCore) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(update["effects"][0]["url"], "https://shop.example/_navree/native/shell");

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
    fn a_bad_argument_is_an_error_not_a_crash() {
        // SAFETY: as above; null pointers are what is being tested.
        unsafe {
            assert!(navree_core_new(std::ptr::null(), 0).is_null());
            let junk = b"{not json";
            assert!(navree_core_new(junk.as_ptr(), junk.len()).is_null());
            assert!(text(navree_core_send(std::ptr::null_mut(), std::ptr::null(), 0)).contains("no core"));

            let core = navree_core_new(CONFIG.as_ptr(), CONFIG.len());
            let unknown = br#"{"t":"teleport"}"#;
            assert!(text(navree_core_send(core, unknown.as_ptr(), unknown.len())).contains("error"));
            assert!(text(navree_core_send(core, std::ptr::null(), 0)).contains("error"));
            let query = br#"{"part":"nowhere"}"#;
            assert!(text(navree_core_view(core, query.as_ptr(), query.len())).contains("error"));
            let gone = br#"{"part":"screen","id":99}"#;
            assert_eq!(text(navree_core_view(core, gone.as_ptr(), gone.len())), "null");
            navree_bytes_free(NavreeBytes {
                ptr: std::ptr::null_mut(),
                len: 0,
            });
            navree_core_free(std::ptr::null_mut());
            navree_core_free(core);
        }
    }
}
