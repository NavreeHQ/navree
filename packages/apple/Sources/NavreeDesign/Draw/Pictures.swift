import SwiftUI

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// When a picture that was loaded is shown.
enum PictureRules {
    /// Whether a picture loaded from `loaded` is shown where `asked` is now
    /// asked for: only its own. A view that comes to stand for another
    /// record (its address changed) shows what stands in a picture's place
    /// until that record's own arrives, and for good when it cannot be had:
    /// never the picture of the record that stood there before.
    static func shows(loaded: URL?, asked: URL?) -> Bool {
        loaded != nil && loaded == asked
    }
}

/// A picture from the network, loaded as the app's requests are (with the
/// session's cookies), and what stands in its place until it comes.
struct RemotePicture<Content: View, Placeholder: View>: View {
    let url: URL?
    @ViewBuilder let content: (Image) -> Content
    @ViewBuilder let placeholder: () -> Placeholder
    @Environment(\.navree) private var context
    /// The picture loaded last, and the address it came from.
    @State private var loaded: Image?
    @State private var from: URL?

    var body: some View {
        Group {
            if let loaded, PictureRules.shows(loaded: from, asked: url) {
                content(loaded)
            } else {
                placeholder()
            }
        }
        .task(id: url) {
            guard let url, let data = await context.picture(url), let image = Self.image(data) else { return }
            // The view asks for another picture by now: this one is not its.
            guard !Task.isCancelled else { return }
            loaded = image
            from = url
        }
    }

    static func image(_ data: Data) -> Image? {
        #if canImport(UIKit)
        UIImage(data: data).map { Image(uiImage: $0) }
        #elseif canImport(AppKit)
        NSImage(data: data).map { Image(nsImage: $0) }
        #endif
    }
}
