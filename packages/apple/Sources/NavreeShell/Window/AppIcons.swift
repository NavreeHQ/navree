import SwiftUI
import os

#if canImport(UIKit)
import UIKit
#endif

/// Which of the app's icons the person may choose among (iOS, iPadOS). The
/// icons are the build's: the app declares its alternate app icon sets
/// (`ASSETCATALOG_COMPILER_ALTERNATE_APPICON_NAMES`), the system writes them
/// in the Info.plist, and the person picks one; the app never changes its
/// icon by itself. A Mac has no alternate icons.
public enum IconRules {
    /// The alternate icons the build declares: the names under
    /// `CFBundleIcons` and `CFBundleIcons~ipad` > `CFBundleAlternateIcons`.
    public static func declared(_ info: [String: Any]) -> Set<String> {
        var names: Set<String> = []
        for key in ["CFBundleIcons", "CFBundleIcons~ipad"] {
            let icons = info[key] as? [String: Any]
            let alternates = icons?["CFBundleAlternateIcons"] as? [String: Any] ?? [:]
            names.formUnion(alternates.keys)
        }
        return names
    }

    /// The icons the person may choose among: the primary icon first, then
    /// each alternate the app names that the build declares, in the app's
    /// order. Without the primary icon there is no way back to it, so nothing
    /// is offered.
    public static func choosable(configured: [AppIcon], declared: Set<String>) -> [AppIcon] {
        guard let primary = configured.first(where: { $0.name == nil }) else { return [] }
        let alternates = configured.filter { icon in icon.name.map(declared.contains) ?? false }
        var seen: Set<String> = []
        return [primary] + alternates.filter { seen.insert($0.id).inserted }
    }

    /// Whether the account menu offers the choice: more than one icon, on a
    /// system that can change it.
    public static func offersChoice(_ choosable: [AppIcon], supportsAlternateIcons: Bool) -> Bool {
        supportsAlternateIcons && choosable.count > 1
    }

    /// The icon chosen now, by its id, from the name of the alternate the
    /// system shows (nil: the primary). An alternate the app no longer offers
    /// marks none.
    public static func selected(alternateIconName: String?, icons: [AppIcon]) -> String? {
        icons.first { $0.name == alternateIconName }?.id
    }
}

/// "App Icon" in the account menu, where the app offers a choice of icons:
/// it opens the choice in a sheet.
struct AppIconMenuItem: View {
    let icons: [AppIcon]
    @Binding var choosing: Bool

    var body: some View {
        #if os(iOS)
        if IconRules.offersChoice(choices, supportsAlternateIcons: UIApplication.shared.supportsAlternateIcons) {
            Section {
                Button("App Icon") { choosing = true }
            }
        }
        #endif
    }

    private var choices: [AppIcon] {
        IconRules.choosable(configured: icons, declared: IconRules.declared(Bundle.main.infoDictionary ?? [:]))
    }
}

/// The sheet the account menu's "App Icon" opens.
struct AppIconChoice: ViewModifier {
    let icons: [AppIcon]
    @Binding var choosing: Bool

    func body(content: Content) -> some View {
        #if os(iOS)
        content.sheet(isPresented: $choosing) {
            AppIconPicker(
                icons: IconRules.choosable(configured: icons, declared: IconRules.declared(Bundle.main.infoDictionary ?? [:]))
            )
        }
        #else
        content
        #endif
    }
}

#if os(iOS)
/// The app's icons as a list to choose from, the chosen one checked, as a
/// glance: at half height, swiped away, Close at the top leading corner. A
/// choice is made at once; the system then says so in its own alert.
struct AppIconPicker: View {
    static let log = Logger(subsystem: "com.navree.native", category: "icon")

    let icons: [AppIcon]
    @State private var chosen: String?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                ForEach(icons) { icon in
                    Button {
                        choose(icon)
                    } label: {
                        HStack(spacing: 16) {
                            Image(icon.preview)
                                .resizable()
                                .scaledToFit()
                                .frame(width: 60, height: 60)
                                .accessibilityHidden(true)
                            Text(icon.label)
                                .foregroundStyle(.primary)
                            Spacer(minLength: 0)
                            if chosen == icon.id {
                                Image(systemName: "checkmark")
                                    .fontWeight(.semibold)
                                    .foregroundStyle(.tint)
                                    .accessibilityHidden(true)
                            }
                        }
                    }
                    .accessibilityAddTraits(chosen == icon.id ? .isSelected : [])
                }
            }
            .navigationTitle("App Icon")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) {
                        dismiss()
                    } label: {
                        Label("Close", systemImage: "xmark")
                    }
                }
            }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            chosen = IconRules.selected(alternateIconName: UIApplication.shared.alternateIconName, icons: icons)
        }
    }

    private func choose(_ icon: AppIcon) {
        guard icon.id != chosen else { return }
        Task {
            do {
                try await UIApplication.shared.setAlternateIconName(icon.name)
                // The icon the system now shows, whatever was asked.
                chosen = IconRules.selected(alternateIconName: UIApplication.shared.alternateIconName, icons: icons)
            } catch {
                Self.log.error("icon not changed: \(error.localizedDescription, privacy: .public)")
            }
        }
    }
}
#endif
