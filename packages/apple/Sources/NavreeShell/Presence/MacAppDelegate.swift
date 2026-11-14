#if os(macOS)
import AppKit

/// What the system tells a Mac app outside any window, passed on to the
/// shell. An app adopts it in its `App`:
///
/// ```swift
/// @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
/// ```
@MainActor
open class NavreeAppDelegate: NSObject, NSApplicationDelegate {}
#endif
