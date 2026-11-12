import Foundation
import NavreeProtocol
import SwiftUI

/// The form a control stands in: the form itself, so a submit can be sent even
/// when the server gave the form no id, and the key its values are kept under.
public struct FormRef: Sendable, Equatable, Hashable {
    /// The form's id, or `@` and its address when it has none.
    public let key: String
    public let method: Method
    public let action: String
    public let hidden: [Pair]

    public init(_ form: Node.Form) {
        key = form.id ?? "@" + form.action
        method = form.method
        action = form.action
        hidden = form.hidden.map { Pair(name: $0.name, value: $0.value) }
    }

    public init(key: String, method: Method = .get, action: String = "", hidden: [Pair] = []) {
        self.key = key
        self.method = method
        self.action = action
        self.hidden = hidden
    }

    /// The values of fields that stand in no form.
    public static let loose = FormRef(key: "")
}

/// A screen pushed on a stack, as the stack's path holds it: its address and
/// how deep it stands, so the same address twice is two screens.
public struct Step: Hashable, Sendable {
    public let address: String
    public let depth: Int

    public init(address: String, depth: Int) {
        self.address = address
        self.depth = depth
    }
}

/// Where a tapped control stands: the form around it and the sheet or dialog
/// around it, so the runtime knows what a submit sends and what a dismiss closes.
public struct Place: Sendable, Equatable {
    public var form: FormRef?
    public var overlay: String?

    public init(form: FormRef? = nil, overlay: String? = nil) {
        self.form = form
        self.overlay = overlay
    }
}

/// What a person did to a drawn part. The drawing never decides what happens
/// next: it hands each of these to one handler, which the runtime answers.
public enum Intent: Sendable, Equatable {
    /// A control's action: a tap on a button, a link, a row, a menu item.
    case act(Action, Place)
    /// A field's values changed.
    case change(form: FormRef, name: String, values: [String])
    /// A region that came empty is on screen: fetch it.
    case appear(region: String)
    /// The end of a list or table is on screen: fetch what comes after.
    case more(target: String, href: String)
    /// The reader searched: the words, sent with the query form of the table
    /// or the screen's search.
    case search(form: FormRef, name: String, text: String)
    /// A sheet or dialog of the screen was dismissed by the system (a swipe
    /// down, a tap outside, the alert's own button).
    case dismissed(overlay: String)
    /// The reader asked for the screen again (pull to refresh).
    case refresh
    /// The reader put away the screen's line of status, or its alert.
    case noticeDismissed
}

/// How the drawing reaches the runtime: one handler for intents, and the
/// values of the forms as the runtime holds them.
public struct DrawingContext: Sendable {
    public var send: @MainActor @Sendable (Intent) -> Void
    /// The values a form's field holds now, or `nil` for those it came with.
    public var values: @MainActor @Sendable (FormRef, String) -> [String]?
    /// Icon shapes by name, from the envelope's table.
    public var icons: [String: IconShape]
    /// The address of the screen, for sharing.
    public var address: String
    /// The origin, which relative addresses of pictures are read against.
    public var base: URL?
    /// How a link to an address is presented, by the shell's route rules.
    public var presentation: @MainActor @Sendable (String) -> Present
    /// How deep the screen being drawn is in its stack: the root is 0.
    public var depth: Int

    public init(
        send: @escaping @MainActor @Sendable (Intent) -> Void,
        values: @escaping @MainActor @Sendable (FormRef, String) -> [String]? = { _, _ in nil },
        icons: [String: IconShape] = [:],
        address: String = "/",
        base: URL? = nil,
        presentation: @escaping @MainActor @Sendable (String) -> Present = { _ in .push },
        depth: Int = 0
    ) {
        self.send = send
        self.values = values
        self.icons = icons
        self.address = address
        self.base = base
        self.presentation = presentation
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
