import SwiftUI

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// A picture from the network, loaded as the app's requests are (with the
/// session's cookies), and what stands in its place until it comes.
struct RemotePicture<Content: View, Placeholder: View>: View {
    let url: URL?
    @ViewBuilder let content: (Image) -> Content
    @ViewBuilder let placeholder: () -> Placeholder
    @Environment(\.navree) private var context
    @State private var loaded: Image?

    var body: some View {
        Group {
            if let loaded {
                content(loaded)
            } else {
                placeholder()
            }
        }
        .task(id: url) {
            guard let url, let data = await context.picture(url) else { return }
            loaded = Self.image(data)
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
