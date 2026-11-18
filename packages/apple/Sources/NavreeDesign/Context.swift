import Foundation
import NavreeProtocol
import SwiftUI

/// The form a control stands in: the form itself, so a submit can be sent even
/// when the server gave the form no id, and the key its values are kept under.
public struct FormRef: Sendable, Equatable, Hashable {
    /// The form's id, or `@` and its address when it has none.
    public let key: String
    public let method: Navree.Method
    public let action: String
    public let hidden: [Navree.Pair]

    public init(_ form: Navree.Form) {
        key = form.id ?? "@" + form.action
        method = form.method
        action = form.action
        hidden = form.hidden.map { Navree.Pair(name: $0.name, value: $0.value) }
    }

    public init(key: String, method: Navree.Method = .get, action: String = "", hidden: [Navree.Pair] = []) {
        self.key = key
        self.method = method
        self.action = action
        self.hidden = hidden
    }

    /// The values of fields that stand in no form.
    public static let loose = FormRef(key: "")
}

extension Navree.Field {
    /// What a field's value and messages are kept under: its id, which the
    /// runtime core makes its own on its screen. Two fields of one form may
    /// share a name (the quantity of each line of an order); they never share
    /// an id. A tree the core has not named (one drawn for a picture) has
    /// only the name.
    public var key: String {
        id ?? name
    }
}

/// An entry of a navigation stack's path: a screen the runtime holds, or a
/// tap on a link that asks the runtime to open one. A tap is never shown: the
/// runtime answers it with the screen it opens, or with nothing.
public enum PathEntry: Hashable, Sendable {
    case screen(Int)
    case tap(Navree.Action, Place)
}

/// Where a tapped control stands: the form around it and the sheet or dialog
/// around it, so the runtime knows what a submit sends and what a dismiss
/// closes; and the question the control asks before it acts.
public struct Place: Sendable, Hashable {
    public var form: FormRef?
    public var overlay: String?
    /// What the control holds, sent with its action as the event's `text`:
    /// the day chosen in a day pager's calendar (`YYYY-MM-DD`).
    public var text: String?
    /// The control's question, as the tree has it, when the person is to be asked first.
    public var ask: Navree.Question?
    /// The id of the part tapped, when it has one.
    public var from: String?

    public init(
        form: FormRef? = nil, overlay: String? = nil, text: String? = nil, ask: Navree.Question? = nil,
        from: String? = nil
    ) {
        self.form = form
        self.overlay = overlay
        self.text = text
        self.ask = ask
        self.from = from
    }
}

/// What a person did to a drawn part. The drawing never decides what happens
/// next: it hands each of these to one handler, which the runtime answers.
public enum Intent: Sendable, Equatable {
    /// A control's action: a tap on a button, a link, a row, a menu item.
    case act(Navree.Action, Place)
    /// A field's values changed. The field is said by its key (its id,
    /// `Navree.Field.key`), never by its name, which another field of the
    /// same form may share.
    case change(form: FormRef, field: String, values: [String])
    /// A region that came empty is on screen.
    case appear(region: String)
    /// A region that failed is to be fetched again.
    case retry(region: String)
    /// The end of a list or table is on screen: fetch what comes after.
    case more(target: String, href: String)
    /// The reader searched: the words, sent with the query form of the table
    /// (in its search field, said by its key) or the screen's search.
    case search(form: FormRef, field: String, text: String)
    /// A sheet or dialog of the screen was dismissed by the system (a swipe
    /// down, a tap outside, the alert's own button).
    case dismissed(overlay: String)
    /// The reader asked for the screen again (pull to refresh).
    case refresh
    /// The reader put away the screen's line of status, or its alert.
    case noticeDismissed
    /// A tab of a set of tabs that has an id was chosen.
    case tab(id: String, index: Int)
    /// The focus moved to the field the screen asked for.
    case focused(serial: Int)
    /// Files were picked for a field, said by its key: the app keeps their
    /// bytes for the form. `keepLocation` is the field's own word
    /// (`keep_location`): without it, the app takes the place a photo was
    /// taken out of it before it is kept.
    case files(form: FormRef, field: String, urls: [URL], keepLocation: Bool = false)
    /// A row of a table was ticked or cleared; without a row, all of them;
    /// with `across`, every record that matches its query.
    case choose(table: String, row: String?, on: Bool, across: Bool)
    /// A row was dragged to stand before another (or at the end) of a list.
    case move(item: String, from: String, to: String, before: String?)
}

/// Whether the rows after a list's last are on their way, or failed to come.
public struct ListStatus: Sendable, Equatable {
    public var loading: Bool
    public var failed: Bool
    /// Every record that matches the table's query is chosen, not only its rows.
    public var across = false

    public init(loading: Bool = false, failed: Bool = false) {
        self.loading = loading
        self.failed = failed
    }
}

/// Whether a part that fetches itself is on its way, or failed to come.
public struct RegionStatus: Sendable, Equatable {
    public var loading: Bool
    public var failed: Bool

    public init(loading: Bool = false, failed: Bool = false) {
        self.loading = loading
        self.failed = failed
    }
}

/// The field a screen asks to take the focus, and the request's number.
public struct FocusRequest: Sendable, Equatable {
    public var target: String
    public var serial: Int

    public init(target: String, serial: Int) {
        self.target = target
        self.serial = serial
    }
}

/// How the drawing reaches the runtime: one handler for intents, and what
/// the runtime holds for the screen: the values of its forms and what is
/// wrong with them, how its lists and regions are coming, where its focus goes.
public struct DrawingContext: Sendable {
    public var send: @MainActor @Sendable (Intent) -> Void
    /// The values a form's field holds now, or `nil` for those it came with.
    /// The field is asked for by its key (its id), never by its name.
    public var values: @MainActor @Sendable (FormRef, String) -> [String]?
    /// What is wrong with a form's field, by its key, or `nil` for what the tree says.
    public var errors: @MainActor @Sendable (FormRef, String) -> [String]?
    /// How the rows after a list's or table's last are coming, by its id.
    public var list: @MainActor @Sendable (String) -> ListStatus
    /// How a region is coming, by its id.
    public var region: @MainActor @Sendable (String) -> RegionStatus
    /// The field the screen asks to take the focus.
    public var focus: FocusRequest?
    /// The id of the part on the screen under this one that this screen was
    /// opened from: the card it grows out of, for a zoom.
    public var from: String?
    /// Icon shapes by name, from the envelope's table.
    public var icons: [String: Navree.IconShape]
    /// The address of the screen, for sharing.
    public var address: String
    /// The origin.
    public var base: URL?
    /// How deep the screen being drawn is in its stack: the root is 0.
    public var depth: Int
    /// The popovers of the screen that are open, by id: the core keeps their
    /// state apart from the tree.
    public var opened: Set<String> = []
    /// The shell's routes, when the core says them: which addresses open as
    /// something other than a pushed screen.
    public var routes: [Navree.Route] = []
    /// Loads a picture's bytes the way the app's requests go: with the
    /// session's cookies, from the one cookie store.
    public var picture: @MainActor @Sendable (URL) async -> Data? = { _ in nil }
    /// The address of a picture the server names (an image's or an avatar's
    /// `src`), or `nil` when the app may not ask for it: then its place
    /// shows the placeholder.
    public var pictureAddress: @Sendable (String) -> URL? = { _ in nil }
    /// What the app itself says at a form's field, by the field's key,
    /// beside what the server says of its value: each file the person picked
    /// for it that was not added, by name and with why.
    public var notes: @MainActor @Sendable (FormRef, String) -> [String] = { _, _ in [] }

    public init(
        send: @escaping @MainActor @Sendable (Intent) -> Void,
        values: @escaping @MainActor @Sendable (FormRef, String) -> [String]? = { _, _ in nil },
        errors: @escaping @MainActor @Sendable (FormRef, String) -> [String]? = { _, _ in nil },
        list: @escaping @MainActor @Sendable (String) -> ListStatus = { _ in ListStatus() },
        region: @escaping @MainActor @Sendable (String) -> RegionStatus = { _ in RegionStatus() },
        focus: FocusRequest? = nil,
        from: String? = nil,
        icons: [String: Navree.IconShape] = [:],
        address: String = "/",
        base: URL? = nil,
        depth: Int = 0
    ) {
        self.send = send
        self.values = values
        self.errors = errors
        self.list = list
        self.region = region
        self.focus = focus
        self.from = from
        self.icons = icons
        self.address = address
        self.base = base
        self.depth = depth
    }

    /// A context that does nothing: for previews and tests.
    public static let inert = DrawingContext(send: { _ in })
}

extension EnvironmentValues {
    /// The runtime's handler and the form values, for every part of a screen.
    @Entry public var navree: DrawingContext = .inert
    /// The form the parts being drawn stand in.
    @Entry var navreeForm: FormRef? = nil
    /// The sheet or dialog the parts being drawn stand in.
    @Entry var navreeOverlay: String? = nil
    /// Whether the parts being drawn are rows of a list (a `List` or a `Form`),
    /// so they draw as rows rather than as free content.
    @Entry var navreeInRows: Bool = false
}
