import Foundation
import UniformTypeIdentifiers

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// How a copy is put on the clipboard.
public struct CopyOptions: Equatable, Sendable {
    /// Kept on this device: not offered to the person's other devices.
    public var localOnly: Bool
    /// How long it stays, in seconds; `nil` for until something replaces it.
    public var expires: TimeInterval?

    public init(localOnly: Bool = false, expires: TimeInterval? = nil) {
        self.localOnly = localOnly
        self.expires = expires
    }
}

/// What a copy asks of the clipboard, as plain rules.
public enum ClipboardRules {
    /// How long a secret stays on the clipboard: a minute.
    public static let secretLife: TimeInterval = 60

    /// A secret (a recovery code, a one-time password) stays on this device
    /// and goes after ``secretLife``; anything else is an ordinary copy.
    public static func options(sensitive: Bool) -> CopyOptions {
        sensitive ? CopyOptions(localOnly: true, expires: secretLife) : CopyOptions()
    }
}

/// The clipboard a `copy` goes to: the system's, or a stand-in in tests.
@MainActor
protocol Clipboard: AnyObject {
    func put(_ text: String, options: CopyOptions)
}

/// The system's clipboard, with what each system offers for a secret.
///
/// On iPhone and iPad the pasteboard takes both wishes itself
/// (`UIPasteboard.OptionsKey.localOnly` and `.expirationDate`): the system
/// keeps the copy off the person's other devices and removes it at the time.
///
/// On a Mac the pasteboard takes the first only
/// (`NSPasteboard.ContentsOptions.currentHostOnly`: "available only on the
/// current device"). It has no expiry of its own, so the app clears the
/// pasteboard itself when the time is up, and only if the pasteboard still
/// holds what the app put there (its change count has not moved). That holds
/// while the app runs: an app quit within the minute leaves the copy where it
/// is, on that Mac alone.
@MainActor
final class SystemClipboard: Clipboard {
    #if !canImport(UIKit)
    /// The clearing of a secret still to come.
    private var clearing: Task<Void, Never>?
    #endif

    func put(_ text: String, options: CopyOptions) {
        #if canImport(UIKit)
        guard options.localOnly || options.expires != nil else {
            UIPasteboard.general.string = text
            return
        }
        var wishes: [UIPasteboard.OptionsKey: Any] = [:]
        if options.localOnly { wishes[.localOnly] = true }
        if let expires = options.expires { wishes[.expirationDate] = Date().addingTimeInterval(expires) }
        UIPasteboard.general.setItems([[UTType.utf8PlainText.identifier: text]], options: wishes)
        #else
        let pasteboard = NSPasteboard.general
        clearing?.cancel()
        clearing = nil
        if options.localOnly {
            pasteboard.prepareForNewContents(with: .currentHostOnly)
        } else {
            pasteboard.clearContents()
        }
        pasteboard.setString(text, forType: .string)
        guard let expires = options.expires else { return }
        let written = pasteboard.changeCount
        clearing = Task { @MainActor in
            try? await Task.sleep(for: .seconds(expires))
            // Still what the app put there: nothing has been copied since.
            if !Task.isCancelled, NSPasteboard.general.changeCount == written {
                NSPasteboard.general.clearContents()
            }
        }
        #endif
    }
}
