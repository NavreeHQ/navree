import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI

/// A navigation stack bound to one of the core's stacks: the core's screens
/// are its path, a tap on a link that pushes asks the core, and the system's
/// back button and swipe tell the core what they took away.
struct StackView: View {
    let stack: [ScreenID]
    let client: NativeClient
    let configuration: ShellConfiguration
    /// The presentation the stack stands in, when it is presented over everything.
    var modal: Modal?

    @Environment(\.navreeWindow) private var window

    var body: some View {
        let layout = modal == nil ? SplitRules.layout(stack.first.flatMap { client.screen($0)?.screen?.body }, width: window) : .stack
        if case .stack = layout {
            stacked
        } else {
            ColumnsStack(layout: layout, stack: stack, client: client, configuration: configuration)
        }
    }

    @ViewBuilder private var stacked: some View {
        if let root = stack.first {
            NavigationStack(path: path) {
                PageView(id: root, depth: 0, modal: modal, client: client, configuration: configuration)
                    .navigationDestination(for: PathEntry.self) { entry in
                        if case .screen(let id) = entry {
                            PageView(
                                id: id, depth: stack.firstIndex(of: id) ?? 1, modal: nil, client: client,
                                configuration: configuration
                            )
                        }
                    }
            }
        }
    }

    private var path: Binding<[PathEntry]> {
        Binding(
            get: { StackPath.path(stack) },
            set: { next in
                switch StackPath.change(stack, to: next) {
                case .tap(let action, let place, let screen)?:
                    client.perform(.act(action, place), on: screen)
                case .popTo(let screen)?:
                    client.send(.popTo(screen: screen))
                case nil:
                    break
                }
            }
        )
    }
}

/// How a navigation stack's path and the core's stack meet.
public enum StackPath {
    /// What the system's path shows of a stack: every screen over the first.
    public static func path(_ stack: [ScreenID]) -> [PathEntry] {
        stack.dropFirst().map(PathEntry.screen)
    }

    public enum Change: Equatable {
        /// A link that pushes was tapped on `screen`: the core is asked, and
        /// the path shows what it answers.
        case tap(Navree.Action, Place, screen: ScreenID)
        /// The back button, the swipe, the long press on back: the stack goes
        /// down to `screen`.
        case popTo(ScreenID)
    }

    /// What the system's new path says happened to `stack`.
    public static func change(_ stack: [ScreenID], to next: [PathEntry]) -> Change? {
        let shown = path(stack)
        guard let top = stack.last else { return nil }
        if next.count > shown.count, case .tap(let action, let place)? = next.last {
            return .tap(action, place, screen: top)
        }
        if next.count < shown.count, Array(shown.prefix(next.count)) == next {
            return .popTo(stack[next.count])
        }
        return nil
    }
}

/// The screens presented over everything, each over the one before: a sheet,
/// a task, or (on iPhone and iPad) a cover.
struct Modals: ViewModifier {
    let level: Int
    let client: NativeClient
    let configuration: ShellConfiguration

    func body(content: Content) -> some View {
        let modal = level < client.nav.modals.count ? client.nav.modals[level] : nil
        let style = modal.map { ModalRules.presentation($0.present) }
        #if os(iOS)
        content
            .sheet(item: binding(modal, shown: style == .sheet)) { _ in presented }
            .fullScreenCover(item: binding(modal, shown: style == .cover)) { _ in presented }
        #else
        content
            .sheet(item: binding(modal, shown: modal != nil)) { _ in presented }
        #endif
    }

    /// The presented stack, with the presentations over it.
    @ViewBuilder private var presented: some View {
        if level < client.nav.modals.count {
            let modal = client.nav.modals[level]
            let height = ModalRules.height(modal)
            StackView(stack: modal.stack, client: client, configuration: configuration, modal: modal)
                .modifier(Modals(level: level + 1, client: client, configuration: configuration))
                .interactiveDismissDisabled(!ModalRules.dismissible(modal, unsaved: unsaved(modal)))
                .presentationDetents(height == .half ? [.medium, .large] : [.large])
                .presentationDragIndicator(height == .half ? .visible : .automatic)
                .modifier(SheetSizing())
        }
    }

    /// Whether the first screen of a task holds input the person would lose.
    private func unsaved(_ modal: Modal) -> Bool {
        guard modal.present == .task, let first = modal.stack.first, let state = client.screen(first) else { return false }
        return Unsaved.holds(state)
    }

    private func binding(_ modal: Modal?, shown: Bool) -> Binding<Presented?> {
        let item = shown ? modal?.stack.first.map(Presented.init) : nil
        return Binding(
            get: { item },
            set: { next in
                if next == nil, let item { client.send(.dismissed(screen: item.id)) }
            }
        )
    }

    struct Presented: Identifiable {
        let id: ScreenID
    }
}

/// How the core's presentations are drawn on Apple's systems.
public enum ModalRules {
    public enum Style: Equatable {
        case sheet
        case cover
    }

    /// `full` covers the screen; a sheet, a task and any word this build does
    /// not know are sheets.
    public static func presentation(_ present: Navree.Present) -> Style {
        present == .full ? .cover : .sheet
    }

    /// A glance rests at half height with a grabber and is swiped away; a
    /// task, and anything locked, takes the full height.
    public static func height(_ modal: Modal) -> SheetHeight {
        modal.present == .sheet && !modal.locked ? .half : .full
    }

    /// Whether the person may swipe it away: never a locked one (a sign-in a
    /// session's end put up), nor a task that holds unsaved input.
    public static func dismissible(_ modal: Modal, unsaved: Bool) -> Bool {
        !modal.locked && !(modal.present == .task && unsaved)
    }

    /// The button that takes a presented screen away, when the screen has no
    /// cancel of its own: a task is cancelled, a sheet closed, a locked one has none.
    public static func dismissal(_ modal: Modal) -> Dismissal? {
        if modal.locked { return nil }
        return modal.present == .task ? .cancel : .close
    }

    public enum Dismissal: Equatable {
        case cancel
        case close
    }
}

/// Whether a screen holds what the person typed and has not sent: a field
/// whose value is not the one the page came with.
public enum Unsaved {
    public static func holds(_ state: ScreenState) -> Bool {
        guard let screen = state.screen else { return false }
        let values = state.forms.values.reduce(into: [String: FieldValue]()) { all, form in
            for (id, field) in form.fields { all[id] = field.value }
        }
        var changed = false
        screen.body.walk { node in
            guard !changed, case .field(let field) = node, let id = field.id, let value = values[id] else { return }
            changed = differs(value, from: field.input)
        }
        return changed
    }

    static func differs(_ value: FieldValue, from input: Navree.Input) -> Bool {
        switch (value, input) {
        case (.flag(let on), .toggle(let toggle)): on != toggle.on
        case (.many(let values), .choices(let choices)): Set(values) != Set(choices.values)
        case (.none, _): !input.initialValues.isEmpty
        case (.text(let text), _): text != (input.initialValues.first ?? "")
        default: false
        }
    }
}

/// One screen of a stack.
struct PageView: View {
    let id: ScreenID
    let depth: Int
    let modal: Modal?
    let client: NativeClient
    let configuration: ShellConfiguration
    /// The part of a screen of panes drawn, when only one is.
    var part: SplitPart?

    var body: some View {
        if let model = client.screens[id] {
            ScreenPage(model: model, depth: depth, modal: modal, client: client, configuration: configuration, part: part)
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// A screen as the core holds it: drawn natively when it holds parts, in the
/// web view inside the native bars when it holds a page, and the system's own
/// layouts while it has nothing and when it failed.
struct ScreenPage: View {
    let model: ScreenModel
    let depth: Int
    let modal: Modal?
    let client: NativeClient
    let configuration: ShellConfiguration
    /// The part of a screen of panes drawn, when only one is: its detail.
    var part: SplitPart?
    @State private var webTitle: String?
    /// The serial of a page the web view could not show.
    @State private var webFailed: Int?

    private var state: ScreenState { model.state }

    var body: some View {
        content
            .environment(\.navree, client.context(for: state.id, depth: depth, base: configuration.base))
            .toolbar { toolbar }
            .modifier(Chromeless(hidden: !state.chrome))
            .modifier(QuestionPresenter(state: state, client: client))
    }

    private var screenDepth: ScreenDepth {
        guard depth == 0 else { return .pushed }
        guard let modal else { return .root }
        return ModalRules.presentation(modal.present) == .cover ? .cover : .sheet
    }

    /// The message the person sees, on the screen in front only.
    private var message: Message? {
        client.nav.front == state.id ? client.messages.first : nil
    }

    @ViewBuilder private var content: some View {
        switch state.content {
        case .native(let screen):
            let shown = Drawable.notice(message)
            ScreenView(
                screen: SplitRules.part(part, of: Drawable.screen(screen, states: state.states)), depth: screenDepth,
                appName: client.app?.name ?? "", notice: shown.notice, failure: shown.failure
            )
        case .web(let address, let title, _, let serial):
            // Only an address of the origin is loaded; any other is a page
            // that failed, as is a page the web view could not show.
            if let url = configuration.site?.inside(address), webFailed != serial {
                WebPageView(
                    url: url, serial: serial, screen: state.id, client: client, configuration: configuration,
                    failed: { webFailed = serial }
                ) {
                    webTitle = $0
                }
                .ignoresSafeArea(.container, edges: .bottom)
                .navigationTitle(webTitle ?? title ?? state.title ?? "")
                .modifier(InlineTitle(mode: NavigationRules.webTitleMode(screenDepth)))
                .modifier(WebMessage(message: message, client: client))
            } else {
                failed(Failure.server) { webFailed = nil }
            }
        case .blank:
            if state.failure != nil, !state.loading {
                failed(state.failure)
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .navigationTitle(state.title ?? "")
                    .modifier(InlineTitle(mode: NavigationRules.titleMode(screenDepth)))
            }
        }
    }

    /// The system's layout for a screen that could not be shown, with "Try
    /// Again", which does `retry` and asks the core for the screen again.
    private func failed(_ failure: Failure?, retry: @escaping () -> Void = {}) -> some View {
        let words = Words.failure(failure)
        return ContentUnavailableView {
            Label(words.title, systemImage: "wifi.exclamationmark")
        } description: {
            Text(words.text)
        } actions: {
            Button("Try Again") {
                retry()
                client.send(.refresh(screen: state.id))
            }
                .buttonStyle(.bordered)
        }
        .navigationTitle(state.title ?? "")
        .modifier(InlineTitle(mode: NavigationRules.titleMode(screenDepth)))
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        if LoadingRules.progressInBar(state) {
            ToolbarItem(placement: .primaryAction) {
                ProgressView()
                    .accessibilityLabel(Text("Loading"))
            }
            .sharedBackgroundVisibility(.hidden)
        }
        if depth == 0, modal == nil, state.chrome, AccountPlacement.inToolbar, let user = client.app?.user {
            ToolbarItem(placement: .primaryAction) {
                AccountMenu(user: user, client: client, icons: configuration.icons).modifier(BarItemLook(.bar))
            }
            ToolbarSpacer(.fixed, placement: .primaryAction)
        }
        if case .web(let address, _, _, _) = state.content, let url = configuration.site?.inside(address) {
            ToolbarItem(placement: .primaryAction) {
                ShareLink(item: url) {
                    Label("Share", systemImage: Symbols.standard(.share) ?? "square.and.arrow.up")
                }
                .modifier(BarItemLook(.bar))
            }
        }
        if depth == 0, let modal, !Self.hasCancel(state), let dismissal = ModalRules.dismissal(modal) {
            ToolbarItem(placement: .cancellationAction) {
                SheetDismissButton(dismissal == .cancel ? .cancel : .close) {
                    client.send(.dismissed(screen: state.id))
                }
            }
        }
    }

    static func hasCancel(_ state: ScreenState) -> Bool {
        state.screen?.actions.contains { node in
            if case .button(let button) = node { return button.role == .cancel }
            return false
        } ?? false
    }
}

/// A message over a page the web view shows, as over any screen: a failure
/// is the system's alert; anything else a line of status at the top of the
/// content, which the person puts away. Never a floating toast.
struct WebMessage: ViewModifier {
    let message: Message?
    let client: NativeClient

    func body(content: Content) -> some View {
        let shown = Drawable.notice(message)
        content
            .safeAreaInset(edge: .top, spacing: 0) {
                if let notice = shown.notice {
                    VStack(spacing: 0) {
                        NoticeRow(notice: notice)
                            .padding(.horizontal)
                            .padding(.vertical, 10)
                        Divider()
                    }
                    .background(.background)
                }
            }
            .modifier(MessageAlert(message: shown.failure == nil ? nil : message, client: client))
    }
}

/// When a screen that already shows something is being loaded again: a
/// spinner in its bar, so the person knows the content will change.
public enum LoadingRules {
    public static func progressInBar(_ state: ScreenState) -> Bool {
        guard state.loading || state.refreshing else { return false }
        switch state.content {
        case .blank: return false
        case .native, .web: return true
        }
    }
}

/// A failure over a page the web view shows: the system's alert, with the
/// message's own button when it has one.
struct MessageAlert: ViewModifier {
    let message: Message?
    let client: NativeClient

    func body(content: Content) -> some View {
        content.alert(
            message?.title ?? message?.text ?? Words.notice(message?.notice),
            // The alert's buttons are its only ways out: each says what became of the message.
            isPresented: Binding(get: { message != nil }, set: { _ in }),
            presenting: message
        ) { message in
            if let action = message.action {
                Button(action) { client.send(.messageAction(id: message.id)) }
            }
            Button("OK", role: .cancel) { client.send(.messageShown(id: message.id)) }
        } message: { message in
            if message.title != nil { Text(message.text ?? Words.notice(message.notice)) }
        }
    }
}

/// A screen the shell's routes show without the app around it, such as sign-in.
struct Chromeless: ViewModifier {
    let hidden: Bool

    func body(content: Content) -> some View {
        #if os(iOS)
        if hidden {
            content.toolbarVisibility(.hidden, for: .navigationBar, .tabBar)
        } else {
            content
        }
        #else
        content
        #endif
    }
}

struct InlineTitle: ViewModifier {
    let mode: TitleMode

    func body(content: Content) -> some View {
        #if os(iOS)
        content.navigationBarTitleDisplayMode(mode == .large ? .large : .inline)
        #else
        content
        #endif
    }
}

/// The person signed in, at the trailing end of a destination's first
/// screen: their name and what their menu offers (profile, sign out), and
/// on iPhone and iPad the app's icon where the app offers a choice of icons.
struct AccountMenu: View {
    let user: Navree.User
    let client: NativeClient
    /// The icons the app offers the person: "App Icon" opens the choice.
    var icons: [AppIcon] = []
    @State private var choosingIcon = false

    var body: some View {
        Menu {
            Section(user.email.map { "\(user.name)\n\($0)" } ?? user.name) {
                ForEach(Array(user.menu.enumerated()), id: \.offset) { _, item in
                    switch item {
                    case .link(let link):
                        Button(link.label) { act(link.action, ask: nil) }
                    case .button(let button):
                        Button(role: button.role == .destructive ? .destructive : nil) {
                            if let action = button.action { act(action, ask: PresentationRules.question(button)) }
                        } label: {
                            Text(button.label)
                        }
                    default:
                        EmptyView()
                    }
                }
            }
            AppIconMenuItem(icons: icons, choosing: $choosingIcon)
        } label: {
            Label(user.name, systemImage: "person.crop.circle")
        }
        .modifier(AppIconChoice(icons: icons, choosing: $choosingIcon))
    }

    /// The account menu stands on no screen.
    private func act(_ action: Navree.Action, ask: Navree.Question?) {
        client.send(.act(.init(screen: nil, action: action, ask: ask, now: Int(Date().timeIntervalSince1970))))
    }
}

/// The question the core asks before a control acts, in the server's words:
/// a destructive act's own question as an action sheet (its answer in red
/// first, Cancel last), any other as an alert. Each button is an answer.
struct QuestionPresenter: ViewModifier {
    let state: ScreenState
    let client: NativeClient

    func body(content: Content) -> some View {
        let question = state.question
        let asker = question.flatMap { question in state.screen.flatMap { QuestionRules.asker(title: question.title, in: $0) } }
        let style = QuestionRules.style(asker: asker)
        content
            .alert(
                question?.title ?? "", isPresented: shown(question, style == .alert), presenting: question
            ) { question in
                answers(question, destructive: false)
            } message: { question in
                if let text = question.text { Text(text) }
            }
            .confirmationDialog(
                question?.title ?? "", isPresented: shown(question, style == .confirmationDialog),
                titleVisibility: .visible, presenting: question
            ) { question in
                answers(question, destructive: true)
            } message: { question in
                if let text = question.text { Text(text) }
            }
    }

    @ViewBuilder private func answers(_ question: Question, destructive: Bool) -> some View {
        Button(question.confirm, role: destructive ? .destructive : nil) {
            client.send(.answer(screen: state.id, question: question.id, yes: true))
        }
        Button(question.cancel ?? String(localized: "Cancel"), role: .cancel) {
            client.send(.answer(screen: state.id, question: question.id, yes: false))
        }
    }

    /// The buttons are its only ways out, and each is an answer.
    private func shown(_ question: Question?, _ style: Bool) -> Binding<Bool> {
        Binding(get: { question != nil && style }, set: { _ in })
    }
}
