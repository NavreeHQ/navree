import NavreeDesign
import NavreeProtocol
import SwiftUI

/// A first screen that proves the package builds, links and draws.
public struct ShellPreview: View {
    @Environment(\.horizontalSizeClass) private var sizeClass

    public init() {}

    public var body: some View {
        NavigationStack {
            List {
                Section("Client") {
                    LabeledContent("Protocol", value: "\(Wire.major)")
                    LabeledContent("Vocabulary", value: "\(Wire.vocab)")
                    LabeledContent("Width", value: WidthClass(sizeClass) == .compact ? "Compact" : "Regular")
                }
            }
            .navigationTitle("Orders")
        }
    }
}
