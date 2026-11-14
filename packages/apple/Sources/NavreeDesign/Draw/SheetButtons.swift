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
    /// Escape presses Cancel. iPhone and iPad sheets keep their own.
    public func sheetKey(_ role: SheetKeyRole) -> some View {
        modifier(SheetKey(role: role))
    }
}

struct SheetKey: ViewModifier {
    let role: SheetKeyRole

    func body(content: Content) -> some View {
        switch PresentationRules.sheetButtons(bar: .current) {
        case .symbols: content
        case .words: content.keyboardShortcut(role == .confirm ? .defaultAction : .cancelAction)
        }
    }
}

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
                .sheetKey(.cancel)
        case .close:
            Button(role: .close, action: action) { SheetButtonLabel("Close", systemImage: "xmark") }
                .sheetKey(.cancel)
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
