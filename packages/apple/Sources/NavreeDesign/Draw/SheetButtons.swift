import SwiftUI

extension EnvironmentValues {
    /// The room a sheet has in the window it stands on: the window's size
    /// under its bar, where the window says it (a Mac's). A Mac sheet takes the
    /// system's size for a form within it.
    @Entry public var navreeSheetRoom: CGSize? = nil
}

/// Words and a symbol, as a sheet's buttons show them on this system: the
/// symbol in an iPhone's or iPad's sheet bar, its words for VoiceOver; the
/// words alone on a Mac sheet's push button.
public struct SheetButtonLabel: View {
    private enum Words {
        case key(LocalizedStringKey)
        case verbatim(String)
    }

    private let words: Words
    private let symbol: String

    public init(_ words: LocalizedStringKey, systemImage symbol: String) {
        self.words = .key(words)
        self.symbol = symbol
    }

    public init(verbatim words: String, systemImage symbol: String) {
        self.words = .verbatim(words)
        self.symbol = symbol
    }

    public var body: some View {
        switch (PresentationRules.sheetButtons(bar: .current), words) {
        case (.symbols, .key(let key)): Label(key, systemImage: symbol)
        case (.symbols, .verbatim(let text)): Label(text, systemImage: symbol)
        case (.words, .key(let key)): Text(key)
        case (.words, .verbatim(let text)): Text(text)
        }
    }
}

/// Which of a sheet's buttons a key presses.
public enum SheetKeyRole: Sendable, Equatable {
    /// The confirming button: Return.
    case confirm
    /// Cancel or Close: Escape.
    case cancel
}

extension View {
    /// The key a sheet's button answers where the system's sheets have words
    /// (a Mac's): Return presses the confirming one, the default button, and
    /// Escape presses Cancel, also on its first press while a field is being
    /// typed in (`action` is what Cancel does). iPhone and iPad sheets keep
    /// their own.
    public func sheetKey(_ role: SheetKeyRole, action: (() -> Void)? = nil) -> some View {
        modifier(SheetKey(role: role, action: action))
    }
}

struct SheetKey: ViewModifier {
    let role: SheetKeyRole
    var action: (() -> Void)?

    func body(content: Content) -> some View {
        switch PresentationRules.sheetButtons(bar: .current) {
        case .symbols: content
        case .words:
            #if os(macOS)
            if role == .cancel, let action {
                content.keyboardShortcut(.cancelAction).background(EscapeWhileTyping(action: action))
            } else {
                content.keyboardShortcut(role == .confirm ? .defaultAction : .cancelAction)
            }
            #else
            content.keyboardShortcut(role == .confirm ? .defaultAction : .cancelAction)
            #endif
        }
    }
}

#if os(macOS)
import AppKit

/// Escape while a field of the sheet is being typed in presses the sheet's
/// Cancel, as in the system's own form sheets
/// (``PresentationRules/escapeWhileTyping(bar:composing:)``): a field in a
/// list row would otherwise take the first press to end its editing, and only
/// a second press would reach Cancel. It looks at the keys of its own window
/// only, and only while a field's editor has the keyboard; otherwise the
/// button's own shortcut answers Escape.
struct EscapeWhileTyping: NSViewRepresentable {
    let action: () -> Void

    func makeNSView(context: Context) -> Watcher { Watcher() }

    func updateNSView(_ view: Watcher, context: Context) { view.action = action }

    static func dismantleNSView(_ view: Watcher, coordinator: ()) { view.stop() }

    final class Watcher: NSView {
        var action: (() -> Void)?
        private var monitor: Any?

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            stop()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                let key = event.keyCode
                let modifiers = event.modifierFlags.intersection([.command, .option, .control, .shift])
                let number = event.windowNumber
                let taken = MainActor.assumeIsolated { self?.takes(key: key, modifiers: modifiers, window: number) ?? false }
                return taken ? nil : event
            }
        }

        /// Escape, alone, in this window, while a field's editor has the keyboard.
        private func takes(key: UInt16, modifiers: NSEvent.ModifierFlags, window number: Int) -> Bool {
            guard key == 53, modifiers.isEmpty, let window, window.windowNumber == number,
                let editor = window.firstResponder as? NSTextView, editor.isFieldEditor,
                PresentationRules.escapeWhileTyping(bar: .window, composing: editor.hasMarkedText()) == .cancelsSheet,
                let action
            else { return false }
            action()
            return true
        }

        func stop() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
        }
    }
}
#endif

/// A sheet's Cancel (what was typed is lost) or Close (nothing is), as the
/// system's sheets have it: ✕ in the bar of an iPhone's or iPad's, its word
/// on a Mac's, which Escape presses.
public struct SheetDismissButton: View {
    let dismissal: SheetDismissal
    let action: () -> Void

    public init(_ dismissal: SheetDismissal, action: @escaping () -> Void) {
        self.dismissal = dismissal
        self.action = action
    }

    public var body: some View {
        switch dismissal {
        case .cancel:
            Button(role: .cancel, action: action) { SheetButtonLabel("Cancel", systemImage: "xmark") }
                .sheetKey(.cancel, action: action)
        case .close:
            Button(role: .close, action: action) { SheetButtonLabel("Close", systemImage: "xmark") }
                .sheetKey(.cancel, action: action)
        }
    }
}

#if os(macOS)
/// The system's size for a form, within the room the window the sheet stands
/// on has under its bar (``PresentationRules/sheetSize(form:room:)``).
struct RoomFormSizing: PresentationSizing {
    let room: CGSize?

    func proposedSize(for root: PresentationSizingRoot, context: PresentationSizingContext) -> ProposedViewSize {
        let form = FormPresentationSizing.form.proposedSize(for: root, context: context)
        guard room != nil else { return form }
        let size = PresentationRules.sheetSize(
            form: CGSize(
                width: form.width ?? PresentationRules.formSize.width, height: form.height ?? PresentationRules.formSize.height
            ),
            room: room
        )
        return ProposedViewSize(size)
    }
}
#endif
