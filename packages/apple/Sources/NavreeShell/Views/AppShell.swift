import NavreeCoreBridge
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// The app's chrome, drawn from the runtime core's view state: its
/// destinations as a tab bar on a compact width and as a tab bar that becomes
/// a sidebar on a regular one (chosen from the window, never from the
/// device), a navigation stack bound to each destination's stack, and the
/// screens the core presents over everything.
public struct AppShell: View {
    @State private var session: Session
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.scenePhase) private var phase
    @Environment(\.colorScheme) private var scheme
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    /// Set while the app is away: the system brings the scene back with it
    /// only when it ended the app itself, never after the person quit it.
    @SceneStorage("navree.restorable") private var restorable = false
    /// This window's own name, so each window keeps what it showed apart.
    @SceneStorage("navree.scene") private var scene = ""
    /// Whether the shell stands in the window under an item in the menu bar.
    @Environment(\.navreeCompactWindow) private var compactWindow
    @State private var shownWidth: CGFloat = 0

    public init(configuration: ShellConfiguration) {
        _session = State(initialValue: Session(configuration: configuration))
    }

    public var body: some View {
        Group {
            if let client = session.client {
                Chrome(client: client, configuration: session.configuration, commands: session.commands)
                    .environment(\.navreeStartsEditing, session.configuration.launch.editing)
                    .onGeometryChange(for: CGFloat.self, of: { $0.size.width }) { width in
                        shownWidth = width
                        session.window(StatusRules.width(compactWindow: compactWindow, sizeClass: WindowRules.system(sizeClass), points: width))
                    }
                    .onChange(of: compactWindow) { _, compact in
                        session.window(StatusRules.width(compactWindow: compact, sizeClass: WindowRules.system(sizeClass), points: shownWidth))
                    }
                    .focusedSceneValue(\.navreeCommands, session.commands)
                    .modifier(OptionalConfirmation(commands: session.commands))
            } else {
                ContentUnavailableView(
                    "This App Can't Start", systemImage: "exclamationmark.triangle",
                    description: Text("Its origin is not one https address.")
                )
            }
        }
        .modifier(PresenceHooks(session: session, scene: scene))
        .modifier(WindowSizing(look: session.configuration.windows))
        .task {
            if scene.isEmpty { scene = UUID().uuidString }
            session.begin(restoring: restorable, scene: scene)
            // A Mac brings its windows back as they were, also after the
            // person quit (unless they asked the system not to): a window it
            // restores starts from what it showed.
            restorable = WindowRules.restoresAfterQuit
            session.environment(traits)
        }
        .onChange(of: traits) { _, traits in session.environment(traits) }
        .onChange(of: phase) { _, phase in
            switch phase {
            case .active:
                session.client?.send(.foreground)
            case .background:
                session.client?.send(.background)
                session.keep()
                restorable = true
            default:
                if WindowRules.restoresAfterQuit { session.keep() }
            }
        }
        #if os(macOS)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
            session.keep()
        }
        #endif
        .onOpenURL { url in session.client?.send(.openLink(url: url.absoluteString)) }
        .onReceive(NotificationCenter.default.publisher(for: AppEvents.pushToken)) { note in
            if let token = note.userInfo?["token"] as? String {
                session.client?.send(.pushToken(token: token, sandbox: AppEvents.sandbox))
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: AppEvents.notification)) { note in
            if let href = note.userInfo?["href"] as? String { session.client?.send(.notification(href: href)) }
        }
        .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
            if let url = activity.webpageURL { session.client?.send(.openLink(url: url.absoluteString)) }
        }
    }

    /// The person's settings as the core hears them.
    private var traits: DeviceTraits {
        var traits = Session.device()
        traits.scheme = scheme == .dark ? .dark : .light
        traits.contrast = contrast == .increased ? .more : .normal
        traits.textPercent = WindowRules.textPercent(typeSize)
        traits.locale = locale.identifier(.bcp47)
        traits.tz = timeZone.identifier
        return traits
    }
}

/// The core, the client over it, and what carries out its effects, for as
/// long as the scene lives.
@MainActor
@Observable
final class Session {
    let configuration: ShellConfiguration
    let client: NativeClient?
    let effects: SystemEffects?
    /// What the menu bar's commands act on in this window.
    let commands: WindowCommands?
    @ObservationIgnored private var heard: DeviceTraits
    @ObservationIgnored private var width: Width?
    @ObservationIgnored private var launched = false
    /// The name what this window keeps is written under.
    @ObservationIgnored private var kept = Session.snapshot

    init(configuration: ShellConfiguration) {
        self.configuration = configuration
        let device = Session.device()
        heard = device
        let config = CoreConfiguration.json(configuration, device: device)
        guard let core = Core(config: config) else {
            client = nil
            effects = nil
            commands = nil
            return
        }
        let effects = SystemEffects(
            transport: configuration.transport(),
            cache: KeptFiles(Self.cacheFolder(configuration.origin), in: .cachesDirectory),
            signedWith: configuration.signedWith, origin: configuration.base, site: configuration.site,
            pictureHosts: configuration.pictureHosts
        )
        self.effects = effects
        let client = NativeClient(core: core, effects: effects)
        self.client = client
        commands = WindowCommands(client: client)
    }

    /// Starts once: from the kept snapshot when the system brings back a
    /// scene it ended, else plainly (and what was kept is let go).
    func begin(restoring: Bool, scene: String = "") {
        guard let client, let effects, !client.began else { return }
        // Each scene runs a core of its own: its pages and its snapshot are kept apart.
        effects.keep(for: scene)
        kept = scene.isEmpty ? Self.snapshot : Self.snapshot + "-" + scene
        let snapshot = restoring ? effects.snapshots.readData(kept) : nil
        if snapshot == nil { effects.snapshots.writeData(kept, nil) }
        client.begin(restoring: snapshot)
        // The token is asked for at every launch of a build signed for push.
        if configuration.signedWith.contains(.pushNotifications) { AppEvents.registerForPush() }
        launch(configuration.launch, in: client)
    }

    /// The app went away: what the core keeps for a launch the system may end.
    func keep() {
        guard let client, let effects else { return }
        effects.snapshots.writeData(kept, client.snapshot())
    }

    func window(_ width: Width) {
        guard width != self.width else { return }
        self.width = width
        client?.send(.window(width: width))
    }

    /// Tells the core what changed of the person's settings, once.
    func environment(_ traits: DeviceTraits) {
        let change = WindowRules.change(from: heard, to: traits)
        heard = traits
        if !change.isEmpty { client?.send(.environment(change)) }
    }

    /// Opens what the launch asks for, as taps, once the app is ready.
    private func launch(_ launch: Launch, in client: NativeClient) {
        guard !launch.isEmpty, !launched else { return }
        if client.nav.stage == .ready {
            launched = true
            return LaunchSteps.run(launch, on: client)
        }
        withObservationTracking {
            _ = client.nav.stage
        } onChange: { [weak self] in
            Task { @MainActor in self?.launch(launch, in: client) }
        }
    }

    static let snapshot = "snapshot"

    /// The folder the core's cache is kept in, one for each origin: what one
    /// origin's answers left is never another's head start.
    nonisolated static func cacheFolder(_ origin: String) -> String {
        let name = origin.lowercased().unicodeScalars
            .map { CharacterSet.alphanumerics.contains($0) ? String($0) : "-" }
            .joined()
        return "navree-cache-" + name
    }

    /// This device as the app starts.
    static func device() -> DeviceTraits {
        #if canImport(UIKit)
        let device = UIDevice.current
        let traits = UITraitCollection.current
        return DeviceTraits(
            platform: .ios, os: device.systemVersion, form: device.userInterfaceIdiom == .pad ? .tablet : .phone,
            scheme: traits.userInterfaceStyle == .dark ? .dark : .light,
            contrast: traits.accessibilityContrast == .high ? .more : .normal,
            textPercent: Int((UIFontMetrics.default.scaledValue(for: 17) / 17 * 100).rounded()),
            locale: Locale.current.identifier(.bcp47), tz: TimeZone.current.identifier
        )
        #else
        let version = ProcessInfo.processInfo.operatingSystemVersion
        return DeviceTraits(
            platform: .macos, os: "\(version.majorVersion).\(version.minorVersion)", form: .desktop,
            locale: Locale.current.identifier(.bcp47), tz: TimeZone.current.identifier
        )
        #endif
    }
}

/// The screens a launch opens, told to the core as taps: the destination
/// chosen, each address followed from the screen in front, a sheet last.
@MainActor
enum LaunchSteps {
    static func run(_ launch: Launch, on client: NativeClient) {
        if let select = launch.select, let index = client.nav.destinations.firstIndex(where: { $0.href == select }) {
            client.send(.select(destination: index))
        }
        for address in launch.push {
            guard let front = client.nav.front else { break }
            client.send(.act(.init(screen: front, action: .navigate(.init(href: address, present: .push)))))
        }
        if let sheet = launch.sheet, let front = client.nav.front {
            client.send(.act(.init(screen: front, action: .navigate(.init(href: sheet, present: .sheet)))))
        }
        if !launch.taps.isEmpty {
            Task { await tap(launch.taps, on: client) }
        }
    }

    /// Each control tapped once the screen in front has come, in turn.
    static func tap(_ labels: [String], on client: NativeClient) async {
        for label in labels {
            for _ in 0..<120 {
                if let front = client.front, !front.loading, let screen = front.screen,
                    let tap = LaunchTaps.find(label, in: screen, open: Set(front.states.filter { $0.value.contains(.open) }.keys))
                {
                    // On a screen of panes a link is a choice, as a row tapped
                    // in its list is: the screen for it takes this one's place.
                    let action = SplitRules.panes(screen.body) == nil ? tap.action : SplitRules.choice(tap.action)
                    client.perform(.act(action, tap.place), on: front.id)
                    break
                }
                try? await Task.sleep(for: .milliseconds(250))
            }
            try? await Task.sleep(for: .milliseconds(800))
        }
    }
}

/// What a window and the person's settings are, in the core's words.
public enum WindowRules {
    /// The window's width class, from the size class the system gives it and
    /// its width in points. Compact as the system's compact size class, and
    /// for any window narrower than a phone held across (a narrow Stage
    /// Manager window), whatever the system says; a regular one is medium
    /// until it is as wide as a tablet held across. A window the system gives
    /// no size class (a Mac's) is never compact: its sidebar folds away by
    /// itself, and a Mac app never takes a phone's tab bar.
    public static func width(sizeClass: WidthClass?, points: CGFloat) -> Width {
        Width(rawValue: WindowClass(sizeClass: sizeClass, points: Double(points)).rawValue) ?? .compact
    }

    /// Whether windows come back as they were after the person quit the app:
    /// a Mac's do; an iPhone's and an iPad's only after the system ended it.
    static var restoresAfterQuit: Bool {
        #if os(macOS)
        true
        #else
        false
        #endif
    }

    /// Whether the destinations are a sidebar that never turns into a tab
    /// bar: a window the system gives no size class (a Mac's).
    static var sidebarOnly: Bool {
        #if os(macOS)
        true
        #else
        false
        #endif
    }

    /// The size class the system gives the window: none on a Mac.
    static func system(_ sizeClass: UserInterfaceSizeClass?) -> WidthClass? {
        #if os(macOS)
        nil
        #else
        WidthClass(sizeClass)
        #endif
    }

    /// The drawing's width, as the core was told it.
    public static func window(_ width: Width) -> WindowClass {
        WindowClass(rawValue: width.rawValue) ?? .compact
    }

    /// The text size as a share of the default, from the body style's size at
    /// each Dynamic Type size.
    public static func textPercent(_ size: DynamicTypeSize) -> Int {
        let body: Double = switch size {
        case .xSmall: 14
        case .small: 15
        case .medium: 16
        case .large: 17
        case .xLarge: 19
        case .xxLarge: 21
        case .xxxLarge: 23
        case .accessibility1: 28
        case .accessibility2: 33
        case .accessibility3: 40
        case .accessibility4: 47
        case .accessibility5: 53
        @unknown default: 17
        }
        return Int((body / 17 * 100).rounded())
    }

    /// What changed of the person's settings, and only that.
    public static func change(from old: DeviceTraits, to new: DeviceTraits) -> CoreEvent.Environment {
        CoreEvent.Environment(
            scheme: old.scheme == new.scheme ? nil : new.scheme,
            contrast: old.contrast == new.contrast ? nil : new.contrast,
            textPercent: old.textPercent == new.textPercent ? nil : new.textPercent,
            locale: old.locale == new.locale ? nil : new.locale,
            tz: old.tz == new.tz ? nil : new.tz
        )
    }
}

/// The app in each of its stages: starting, failed, too old, ready.
struct Chrome: View {
    let client: NativeClient
    let configuration: ShellConfiguration
    var commands: WindowCommands?
    /// In the window under an item in the menu bar the destinations' sidebar
    /// starts folded away, behind the toolbar's sidebar button.
    @Environment(\.navreeCompactWindow) private var compactWindow
    @State private var columns: NavigationSplitViewVisibility = .automatic

    var body: some View {
        switch client.nav.stage {
        case .starting:
            ProgressView()
                .controlSize(.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed:
            let words = Words.failure(client.nav.failure)
            ContentUnavailableView {
                Label(words.title, systemImage: "wifi.exclamationmark")
            } description: {
                Text(words.text)
            } actions: {
                Button("Try Again") { client.send(.start) }
                    .buttonStyle(.borderedProminent)
            }
        case .upgrade:
            ContentUnavailableView {
                Label("Update Required", systemImage: "arrow.down.app")
            } description: {
                Text("This version of the app is too old for the server. Version \(client.nav.upgrade?.minApp ?? "") or later is needed.")
            } actions: {
                Button("Get the Update") { client.send(.openUpgrade) }
                    .buttonStyle(.borderedProminent)
            }
        case .ready:
            destinations
                .modifier(Modals(level: 0, client: client, configuration: configuration))
                .tint(TintRules.appliesAppAccent(bar: .current) ? ThemeColor.accent(client.app?.theme) : nil)
                .modifier(WindowWidth(width: WindowRules.window(client.nav.width)))
                .environment(\.navreeFind, commands?.finding ?? 0)
        }
    }

    @ViewBuilder private var destinations: some View {
        let items = client.nav.destinations
        let shown = items.indices.contains(client.nav.selected) ? items[client.nav.selected].stack : []
        let stack = SplitRules.layout(
            shown.first.flatMap { client.screen($0)?.screen?.body }, width: WindowRules.window(client.nav.width)
        )
        switch NavigationRules.window(destinations: items.count, sidebarOnly: WindowRules.sidebarOnly, stack: stack) {
        case .sidebarListDetail:
            ColumnsStack(
                layout: stack, stack: shown, client: client, configuration: configuration,
                sidebar: DestinationSidebar(client: client)
            )
        case .sidebarStack:
            NavigationSplitView(columnVisibility: $columns) {
                DestinationSidebar(client: client)
                    .navigationSplitViewColumnWidth(min: 180, ideal: 220, max: 320)
            } detail: {
                StackView(stack: shown, client: client, configuration: configuration)
            }
            .onChange(of: compactWindow, initial: true) { _, compact in columns = compact ? .detailOnly : .automatic }
        case .tabs:
            tabs
        }
    }

    @ViewBuilder private var tabs: some View {
        let items = client.nav.destinations
        let groups = Set(items.compactMap(\.group)).count
        switch NavigationRules.structure(destinations: items.count, width: WindowRules.window(client.nav.width).widthClass) {
        case .single:
            if let first = items.first {
                StackView(stack: first.stack, client: client, configuration: configuration)
            }
        case .tabBar:
            TabView(selection: selection) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    tab(item, index, inTabBar: true)
                }
            }
        case .sidebarAdaptable where !NavigationRules.groupsAsSections(groups):
            TabView(selection: selection) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    tab(item, index)
                }
            }
            .tabViewStyle(.sidebarAdaptable)
            .modifier(SidebarAccount(client: client))
            .id(columnDestinations)
        case .sidebarAdaptable:
            TabView(selection: selection) {
                ForEach(DestinationGroup.of(items)) { group in
                    TabSection(group.name) {
                        ForEach(group.members) { member in
                            tab(member.item, member.index)
                        }
                    }
                }
            }
            .tabViewStyle(.sidebarAdaptable)
            .modifier(SidebarAccount(client: client))
            .id(columnDestinations)
        }
    }

    /// The destinations drawn as columns so far: the tab view is built anew
    /// when one of them first is (see ``SplitRules/columnDestinations(_:)``).
    private var columnDestinations: [Int] {
        let width = WindowRules.window(client.nav.width)
        return SplitRules.columnDestinations(client.nav.destinations.map { item in
            SplitRules.layout(item.stack.first.flatMap { client.screen($0)?.screen?.body }, width: width)
        })
    }

    private func tab(_ item: Destination, _ index: Int, inTabBar: Bool = false) -> some TabContent<Int> {
        Tab(value: index) {
            StackView(stack: item.stack, client: client, configuration: configuration)
        } label: {
            DestinationLabel(label: item.label, icon: item.icon, inTabBar: inTabBar)
        }
        .badge(BadgeRules.tabBadge(item.badge))
    }

    private var selection: Binding<Int> {
        Binding(get: { client.nav.selected }, set: { client.send(.select(destination: $0)) })
    }
}

/// The destinations of a group, in the order the groups first come.
struct DestinationGroup: Identifiable {
    struct Member: Identifiable {
        let index: Int
        let item: Destination
        var id: Int { index }
    }

    let name: String
    var members: [Member]
    var id: String { name }

    static func of(_ items: [Destination]) -> [DestinationGroup] {
        var groups: [DestinationGroup] = []
        for (index, item) in items.enumerated() {
            let name = item.group ?? ""
            if let at = groups.firstIndex(where: { $0.name == name }) {
                groups[at].members.append(Member(index: index, item: item))
            } else {
                groups.append(DestinationGroup(name: name, members: [Member(index: index, item: item)]))
            }
        }
        return groups
    }
}

/// A destination's label in the tab bar or the sidebar: its name and the
/// system symbol that means the same, or its own icon drawn as a template
/// as large as the system's symbols there.
struct DestinationLabel: View {
    let label: String
    let icon: Navree.Icon?
    /// In a tab bar rather than a sidebar.
    var inTabBar = false

    @Environment(\.navree) private var context
    @Environment(\.legibilityWeight) private var legibility
    #if os(macOS)
    @Environment(\.sidebarRowSize) private var rowSize
    #endif

    var body: some View {
        if let icon, let symbol = Symbols.image(for: icon) {
            Label { Text(label) } icon: { symbol }
        } else if let icon, let image = IconImage.image(icon, icons: context.icons, in: bar, boldText: legibility == .bold) {
            Label { Text(label) } icon: { image }
        } else {
            Label(label, systemImage: "square.dashed")
        }
    }

    private var bar: DrawnIcon.Bar {
        #if os(macOS)
        switch rowSize {
        case .small: .macSidebar(.small)
        case .large: .macSidebar(.large)
        default: .macSidebar(.medium)
        }
        #else
        inTabBar ? .tabBar : .sidebar
        #endif
    }
}

/// The brand's accent as the app's tint, in its light and dark forms. Bars,
/// materials and type stay the system's.
enum ThemeColor {
    static func accent(_ theme: Navree.Theme?) -> Color? {
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

/// The window's width, as the core was told it, for the drawing and for the
/// system's own components: a window the core was told is compact draws as
/// compact everywhere, whatever the device.
struct WindowWidth: ViewModifier {
    let width: WindowClass

    func body(content: Content) -> some View {
        content
            .environment(\.navreeWindow, width)
            .environment(\.horizontalSizeClass, width == .compact ? .compact : .regular)
    }
}

/// Where the person's account stands: a button at the trailing end of each
/// destination's first screen on iPhone and iPad; on a Mac, the App menu and
/// the foot of the sidebar, never the toolbar.
enum AccountPlacement {
    static var inToolbar: Bool {
        #if os(macOS)
        false
        #else
        true
        #endif
    }
}

/// The account at the foot of a Mac's sidebar, as Mail and Music keep it.
struct SidebarAccount: ViewModifier {
    let client: NativeClient

    func body(content: Content) -> some View {
        #if os(macOS)
        if let user = client.app?.user {
            content.tabViewSidebarFooter {
                AccountMenu(user: user, client: client)
                    .menuStyle(.button)
                    .buttonStyle(.borderless)
                    .labelStyle(.titleAndIcon)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 6)
            }
        } else {
            content
        }
        #else
        content
        #endif
    }
}

/// The question a destructive command of the menu bar asks, when the window has commands.
struct OptionalConfirmation: ViewModifier {
    let commands: WindowCommands?

    func body(content: Content) -> some View {
        if let commands {
            content.modifier(CommandConfirmation(commands: commands))
        } else {
            content
        }
    }
}
