import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

/// What the menu bar acts on: the window in front, the runtime client that
/// draws it, and a destructive command waiting for the person to say yes.
@MainActor
@Observable
public final class WindowCommands {
    let client: NativeClient

    /// A command that destroys, chosen from the menu bar or with its
    /// shortcut, asked about before it is sent: a shortcut is easier to press
    /// by mistake than a button.
    var confirming: Confirming?

    /// Asks the search field of the screen in front to take the focus: a
    /// count, so each ⌘F is a change.
    var finding = 0

    struct Confirming: Identifiable {
        let id = UUID()
        let title: String
        let text: String?
        let confirm: String
        let action: Navree.Action
        let screen: ScreenID
    }

    init(client: NativeClient) {
        self.client = client
    }

    /// The screen the person is looking at, when it shows parts.
    var front: ScreenState? { client.front }

    /// The list a detail stands beside in the window, while a screen is open
    /// beside it: the first screen of the destination, when its stack is drawn
    /// as columns and holds more than it.
    var besideList: ScreenID? {
        let nav = client.nav
        guard nav.modals.isEmpty, nav.destinations.indices.contains(nav.selected) else { return nil }
        let stack = nav.destinations[nav.selected].stack
        guard stack.count > 1, let root = client.screen(stack[0])?.screen else { return nil }
        switch SplitRules.layout(root.body, width: WindowRules.window(nav.width)) {
        case .panes, .listBesideDetail: return stack[0]
        case .stack: return nil
        }
    }

    /// The window's commands, one by one, with the screen each acts on: the
    /// front screen's own actions, and while a detail stands beside its list,
    /// the list's too (its making something new keeping ⌘N).
    var commands: [(node: Navree.Node, screen: ScreenID)] {
        guard let shown = client.nav.front else { return [] }
        // A page's head row is the screen's actions, as its bar has them.
        let depth: ScreenDepth = client.nav.modals.isEmpty ? .pushed : .sheet
        let own = CommandRules.leaves(front?.screen.map { ActionRules.lifted($0, depth: depth).actions } ?? [])
        guard let list = besideList, list != shown, let root = client.screen(list)?.screen else {
            return own.map { ($0, shown) }
        }
        let listed = CommandRules.leaves(SplitRules.part(.list, of: root).actions)
        return CommandRules.window(detail: own, list: listed).map { ($0.node, $0.fromList ? list : shown) }
    }

    /// The window's commands' actions, in order.
    var screenActions: [Navree.Node] { commands.map(\.node) }

    /// Whether Find has a search field to go to: the front screen's, or the
    /// list's beside it.
    var searches: Bool {
        if let screen = front?.screen, CommandRules.searches(screen) { return true }
        guard let list = besideList, let root = client.screen(list)?.screen else { return false }
        return CommandRules.searches(SplitRules.part(.list, of: root))
    }

    /// The row whose detail the window shows beside its list, with its context
    /// menu's actions: the commands for what is chosen.
    var chosen: (title: String, actions: [Navree.Node], screen: ScreenID)? {
        let nav = client.nav
        guard nav.modals.isEmpty, nav.destinations.indices.contains(nav.selected) else { return nil }
        let stack = nav.destinations[nav.selected].stack
        guard stack.count > 1, let root = client.screen(stack[0])?.screen, let detail = client.screen(stack[1]) else {
            return nil
        }
        guard let row = SplitRules.chosenRow(in: root.body, opened: detail.url) else { return nil }
        return (row.title, CommandRules.leaves(row.actions), stack[0])
    }

    /// Carries out an action as the button or row it stands for would: the
    /// server's own question first where it has one; where it has none and
    /// the action destroys what cannot be undone, the client asks.
    func perform(_ node: Navree.Node, on screen: ScreenID) {
        switch node {
        case .button(let button):
            guard let action = button.action else { return }
            if PresentationRules.asksFirst(button), PresentationRules.question(button) == nil {
                confirming = Confirming(
                    title: CommandRules.title(button.label) + "?", text: nil, confirm: button.label, action: action,
                    screen: screen
                )
                return
            }
            client.perform(.act(action, Place(ask: PresentationRules.question(button))), on: screen)
        case .link(let link):
            client.perform(.act(link.action, Place()), on: screen)
        default:
            break
        }
    }
}

extension FocusedValues {
    /// The commands of the window in front.
    @Entry var navreeCommands: WindowCommands?
}

/// The app's commands in the menu bar, for the window in front: every action
/// its toolbar offers (and, while a detail stands beside its list, the list's
/// too) and every action of the chosen row's context menu, with
/// the shortcuts Apple's apps give the standard ones (⌘N new, ⌘⌫ delete, ⌘R
/// refresh, ⌘[ back, ⌘1 to ⌘9 for the destinations), the account's menu in
/// the App menu, and the system's sidebar, toolbar and inspector commands.
///
///     WindowGroup(id: "main") { AppShell(configuration: configuration) }
///         .commands { NavreeCommands(window: "main", configuration: configuration) }
///
/// Given the app's configuration, About and Help are as it says; without it
/// they are the system's own.
public struct NavreeCommands: Commands {
    /// The id of the app's window group, for New Window.
    let window: String
    /// The app's configuration, for About and Help.
    let configuration: ShellConfiguration?
    @FocusedValue(\.navreeCommands) private var target

    public init(window: String) {
        self.window = window
        configuration = nil
    }

    public init(window: String, configuration: ShellConfiguration) {
        self.window = window
        self.configuration = configuration
    }

    public var body: some Commands {
        SidebarCommands()
        ToolbarCommands()
        InspectorCommands()
        // Each group's items are views, so they follow the window's screen
        // as it changes, not only the window that is in front.
        CommandGroup(replacing: .newItem) {
            NewCommands(target: target, window: window)
        }
        CommandGroup(before: .printItem) {
            SlotCommands(target: target, slot: .share)
        }
        CommandGroup(before: .systemServices) {
            AccountCommands(target: target)
        }
        CommandGroup(after: .textEditing) {
            FindCommands(target: target)
        }
        CommandGroup(before: .sidebar) {
            ViewCommands(target: target)
        }
        CommandMenu(CommandRules.screenMenu(destinationLabel)) {
            ScreenCommands(target: target)
        }
        AboutCommands(configuration: configuration)
        HelpCommands(configuration: configuration)
    }

    private var destinationLabel: String? {
        guard let nav = target?.client.nav, nav.destinations.indices.contains(nav.selected) else { return nil }
        return nav.destinations[nav.selected].label
    }
}

/// The front screen's commands of one place in the menu bar.
struct SlotCommands: View {
    let target: WindowCommands?
    let slot: MenuSlot

    var body: some View {
        if let target {
            let commands = target.commands
            ForEach(Array(CommandRules.entries(commands.map(\.node)).filter { $0.slot == slot }.enumerated()), id: \.offset) { _, entry in
                CommandItem(entry: entry) { target.perform(commands[entry.index].node, on: commands[entry.index].screen) }
            }
        }
    }
}

/// File's first group: what the screen makes new (⌘N for the first), and a
/// new window (⌘N where nothing else takes it, else ⌥⌘N, as in Mail).
struct NewCommands: View {
    let target: WindowCommands?
    let window: String
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let creates = target.map { CommandRules.entries($0.screenActions).contains { $0.slot == .new } } ?? false
        SlotCommands(target: target, slot: .new)
        Button("New Window") { openWindow(id: window) }
            .keyboardShortcut(KeyCommand.equivalent(creates ? .newWindow : .new))
    }
}

/// Edit > Find: the screen's own search command, or Find… (⌘F) where the
/// screen has a search field and no such command.
struct FindCommands: View {
    let target: WindowCommands?

    var body: some View {
        if let target {
            let finds = CommandRules.entries(target.screenActions).contains { $0.slot == .find }
            SlotCommands(target: target, slot: .find)
            if !finds, target.searches {
                Button("Find…") { target.finding += 1 }
                    .keyboardShortcut(KeyCommand.equivalent(.find))
            }
        }
    }
}

/// One command: its words, its symbol for a standard action, its shortcut.
struct CommandItem: View {
    let entry: MenuEntry
    let run: () -> Void

    var body: some View {
        Button(role: entry.slot == .destructive ? .destructive : nil, action: run) {
            if let symbol = entry.symbol {
                Label(entry.title, systemImage: symbol)
            } else {
                Text(entry.title)
            }
        }
        .disabled(entry.disabled)
        .modifier(OptionalShortcut(shortcut: entry.shortcut))
    }
}

struct OptionalShortcut: ViewModifier {
    let shortcut: Shortcut?

    func body(content: Content) -> some View {
        if let shortcut {
            content.keyboardShortcut(KeyCommand.equivalent(shortcut))
        } else {
            content
        }
    }
}

/// The View menu's own part: each destination (⌘1 to ⌘9), going back, and
/// fetching the screen again.
struct ViewCommands: View {
    let target: WindowCommands?

    var body: some View {
        if let target {
            let nav = target.client.nav
            ForEach(Array(nav.destinations.enumerated()), id: \.offset) { index, destination in
                Toggle(isOn: Binding(get: { nav.selected == index }, set: { _ in target.client.send(.select(destination: index)) })) {
                    Text(destination.label)
                }
                .modifier(OptionalShortcut(shortcut: CommandRules.destination(index)))
            }
            Divider()
            Button("Back") { target.client.send(.back) }
                .keyboardShortcut(KeyCommand.equivalent(.back))
                .disabled(!nav.canGoBack)
            Button("Refresh") {
                if let front = nav.front { target.client.send(.refresh(screen: front)) }
            }
            .keyboardShortcut(KeyCommand.equivalent(.refresh))
            .disabled(nav.front == nil)
            Divider()
        }
    }
}

/// The menu named for what the window shows: the front screen's actions that
/// have no place of their own, then the chosen row's, then what destroys.
struct ScreenCommands: View {
    let target: WindowCommands?

    var body: some View {
        if let target {
            let commands = target.commands
            let entries = CommandRules.entries(commands.map(\.node))
            ForEach(Array(entries.filter { $0.slot == .screen }.enumerated()), id: \.offset) { _, entry in
                CommandItem(entry: entry) { target.perform(commands[entry.index].node, on: commands[entry.index].screen) }
            }
            if let chosen = target.chosen, !chosen.actions.isEmpty {
                let rows = CommandRules.entries(chosen.actions)
                Section(chosen.title) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, entry in
                        CommandItem(entry: entry) { target.perform(chosen.actions[entry.index], on: chosen.screen) }
                    }
                }
            }
            let destructive = entries.filter { $0.slot == .destructive }
            if !destructive.isEmpty {
                Divider()
                ForEach(Array(destructive.enumerated()), id: \.offset) { _, entry in
                    CommandItem(entry: entry) { target.perform(commands[entry.index].node, on: commands[entry.index].screen) }
                }
            }
        }
    }
}

/// The account's menu in the App menu, after Settings: the person signed in
/// and what their menu offers (their profile, signing out). The entry that
/// leads to their settings is Settings itself.
struct AccountCommands: View {
    let target: WindowCommands?

    var body: some View {
        if let target, let user = target.client.app?.user {
            let settings = SettingsRules.destination(in: user.menu)?.index
            Section(user.name) {
                ForEach(Array(user.menu.enumerated()), id: \.offset) { index, node in
                    if index != settings {
                        switch node {
                        case .link(let link):
                            Button(CommandRules.title(link.label)) { act(link.action, nil, target) }
                        case .button(let button):
                            Button(role: button.role == .destructive ? .destructive : nil) {
                                if let action = button.action { act(action, PresentationRules.question(button), target) }
                            } label: {
                                Text(CommandRules.title(button.label))
                            }
                        default:
                            EmptyView()
                        }
                    }
                }
            }
        }
    }

    /// The account's menu stands on no screen.
    private func act(_ action: Navree.Action, _ ask: Navree.Question?, _ target: WindowCommands) {
        target.client.send(.act(.init(screen: nil, action: action, ask: ask, now: Int(Date().timeIntervalSince1970))))
    }
}

/// The key of a shortcut, as SwiftUI names it.
enum KeyCommand {
    static func equivalent(_ shortcut: Shortcut) -> KeyboardShortcut {
        let modifiers: EventModifiers = shortcut.option ? [.command, .option] : .command
        switch shortcut.key {
        case .letter(let letter): return KeyboardShortcut(KeyEquivalent(letter), modifiers: modifiers)
        case .digit(let digit): return KeyboardShortcut(KeyEquivalent(Character(String(digit))), modifiers: modifiers)
        case .delete: return KeyboardShortcut(.delete, modifiers: modifiers)
        case .leftBracket: return KeyboardShortcut("[", modifiers: modifiers)
        }
    }
}

/// The question a destructive command chosen from the menu bar asks, over the
/// window it acts in: the destructive choice first, Cancel last.
struct CommandConfirmation: ViewModifier {
    @Bindable var commands: WindowCommands

    func body(content: Content) -> some View {
        content.confirmationDialog(
            commands.confirming?.title ?? "",
            isPresented: Binding(get: { commands.confirming != nil }, set: { if !$0 { commands.confirming = nil } }),
            titleVisibility: .visible,
            presenting: commands.confirming
        ) { confirming in
            Button(CommandRules.title(confirming.confirm), role: .destructive) {
                commands.client.perform(.act(confirming.action, Place()), on: confirming.screen)
                commands.confirming = nil
            }
            Button("Cancel", role: .cancel) { commands.confirming = nil }
        } message: { confirming in
            if let text = confirming.text { Text(text) }
        }
    }
}
