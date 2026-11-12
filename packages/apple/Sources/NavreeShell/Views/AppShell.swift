import NavreeDesign
import NavreeProtocol
import SwiftUI

/// The app's chrome, drawn from the shell document: its destinations as a tab
/// bar on a compact width and as a tab bar that becomes a sidebar on a
/// regular one (chosen from the window, never from the device), a navigation
/// stack for each, a sheet and a cover over everything.
public struct AppShell: View {
    @State private var client: NativeClient
    @Environment(\.horizontalSizeClass) private var sizeClass

    public init(configuration: ShellConfiguration) {
        _client = State(initialValue: NativeClient(configuration: configuration))
    }

    public init(client: NativeClient) {
        _client = State(initialValue: client)
    }

    public var body: some View {
        let state = client.state
        let width = WidthClass(sizeClass)
        destinations(state: state, width: width)
            .sheet(isPresented: presented(.sheet)) {
                StackView(container: .sheet, client: client)
            }
            .modifier(Cover(isPresented: presented(.cover), client: client))
            .task { client.start() }
            .onOpenURL { client.send(.deepLink($0)) }
            .tint(ThemeColor.accent(state.shell?.theme))
    }

    @ViewBuilder private func destinations(state: ViewState, width: WidthClass) -> some View {
        let items = state.destinations
        switch NavigationRules.structure(destinations: items.count, width: width) {
        case .single:
            StackView(container: .destination(items.first?.href ?? state.selected), client: client)
        case .tabBar:
            TabView(selection: selection) {
                ForEach(items, id: \.href) { item in
                    Tab(value: item.href) {
                        StackView(container: .destination(item.href), client: client)
                    } label: {
                        DestinationLabel(item: item)
                    }
                    .badge(item.badge.flatMap { Int($0) } ?? 0)
                }
            }
        case .sidebarAdaptable:
            TabView(selection: selection) {
                ForEach(Array((state.shell?.nav.groups ?? []).enumerated()), id: \.offset) { _, group in
                    TabSection(group.label ?? "") {
                        ForEach(group.items, id: \.href) { item in
                            Tab(value: item.href) {
                                StackView(container: .destination(item.href), client: client)
                            } label: {
                                DestinationLabel(item: item)
                            }
                            .badge(item.badge.flatMap { Int($0) } ?? 0)
                        }
                    }
                }
            }
            .tabViewStyle(.sidebarAdaptable)
        }
    }

    private var selection: Binding<String> {
        Binding(get: { client.state.selected }, set: { client.send(.select($0)) })
    }

    private func presented(_ container: Container) -> Binding<Bool> {
        Binding(
            get: { !(client.state.stacks[container] ?? []).isEmpty },
            set: { shown in if !shown { client.send(.dismissed(container)) } }
        )
    }
}

/// A destination's label in the tab bar or the sidebar: its name and the
/// system symbol that means the same, or its own icon drawn as a template.
struct DestinationLabel: View {
    let item: NavItem

    var body: some View {
        if let icon = item.icon, let symbol = Symbols.symbol(for: icon) {
            Label(item.label, systemImage: symbol)
        } else if let icon = item.icon, let image = IconImage.image(icon) {
            Label { Text(item.label) } icon: { image }
        } else {
            Label(item.label, systemImage: "square.dashed")
        }
    }
}

/// The cover over everything: sign-in and full-screen tasks. A Mac has no
/// such thing, and gets a sheet.
struct Cover: ViewModifier {
    let isPresented: Binding<Bool>
    let client: NativeClient

    func body(content: Content) -> some View {
        #if os(iOS)
        content.fullScreenCover(isPresented: isPresented) {
            StackView(container: .cover, client: client)
        }
        #else
        content.sheet(isPresented: isPresented) {
            StackView(container: .cover, client: client)
        }
        #endif
    }
}

/// A navigation stack of screens: the runtime's stack is its path, and the
/// system's back button and swipe tell the runtime what they took away.
struct StackView: View {
    let container: Container
    let client: NativeClient

    var body: some View {
        NavigationStack(path: path) {
            PageView(ref: PageRef(container, 0), client: client)
                .navigationDestination(for: Step.self) { step in
                    PageView(ref: PageRef(container, step.depth), client: client)
                }
        }
    }

    private var path: Binding<[Step]> {
        Binding(
            get: {
                let stack = client.state.stacks[container] ?? []
                return stack.enumerated().dropFirst().map { Step(address: $0.element.address, depth: $0.offset) }
            },
            set: { client.send(.path(container, $0)) }
        )
    }
}

/// The brand's accent as the app's tint, in its light and dark forms. Bars,
/// materials and type stay the system's.
enum ThemeColor {
    static func accent(_ theme: Theme?) -> Color? {
        guard let light = theme?.accent.flatMap(rgb) else { return nil }
        let dark = theme?.accentDark.flatMap(rgb) ?? light
        #if canImport(UIKit)
        return Color(UIColor { traits in
            let value = traits.userInterfaceStyle == .dark ? dark : light
            return UIColor(red: value.0, green: value.1, blue: value.2, alpha: 1)
        })
        #else
        return Color(red: light.0, green: light.1, blue: light.2)
        #endif
    }

    static func rgb(_ hex: String) -> (Double, Double, Double)? {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        return (Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255, Double(value & 0xFF) / 255)
    }
}
