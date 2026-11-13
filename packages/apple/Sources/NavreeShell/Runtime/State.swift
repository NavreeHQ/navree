import Foundation
import NavreeDesign
import NavreeProtocol

// The boundary between the drawing and the client's behaviour: the drawing
// reads a `ViewState` and says what happened with one `Event`; the runtime
// answers with the next state and the `Effect`s to carry out. The shapes
// follow the shared runtime core's events, effects and view state, so the
// core can take this boundary over without the drawing changing.

/// Where a stack of screens lives.
public enum Container: Hashable, Sendable {
    /// One of the app's destinations, by its address.
    case destination(String)
    /// The sheet over everything (a route rule, or a link that asks for it).
    case sheet
    /// The cover over everything: sign-in, a full-screen task.
    case cover
}

/// A screen: its stack and its place in it (0 is the stack's first).
public struct PageRef: Hashable, Sendable {
    public var container: Container
    public var index: Int

    public init(_ container: Container, _ index: Int) {
        self.container = container
        self.index = index
    }
}

/// What a screen holds.
public enum PageContent: Sendable, Equatable {
    case loading
    /// Parts to draw natively.
    case screen(Navree.Screen)
    /// The page shown whole in the web view, inside native chrome.
    case web(URL, title: String?)
    /// It could not be loaded: the words say why.
    case failed(String)
    /// The app is too old for the server.
    case upgrade(Navree.Upgrade)
}

/// One screen of a stack.
public struct PageState: Sendable, Equatable {
    public var address: String
    public var content: PageContent
    /// Icon shapes by name, from the envelope.
    public var icons: [String: Navree.IconShape] = [:]
    /// What the forms of the screen hold, by form then field.
    public var values: [String: [String: [String]]] = [:]
    /// A line of status the screen shows (what the web shows as a toast).
    public var notice: Notice?
    /// A failure the person must see.
    public var failure: Notice?
    /// The title the web page in the web view gave itself.
    public var webTitle: String?
    /// Regions and pages already asked for, so a region is fetched once.
    public var asked: Set<String> = []

    public init(address: String, content: PageContent = .loading) {
        self.address = address
        self.content = content
    }

    /// The title the bar shows.
    public var title: String {
        switch content {
        case .screen(let screen): screen.title
        case .web(_, let title): webTitle ?? title ?? ""
        default: ""
        }
    }
}

/// Everything the drawing needs, and nothing it does not.
public struct ViewState: Sendable, Equatable {
    public var shell: Navree.Shell?
    /// The destination shown, by its address.
    public var selected: String = "/"
    public var stacks: [Container: [PageState]] = [:]
    /// The screen to show again once the person has signed in.
    public var afterSignIn: PageRef?

    public init() {}

    public func page(_ ref: PageRef) -> PageState? {
        guard let stack = stacks[ref.container], ref.index < stack.count else { return nil }
        return stack[ref.index]
    }

    /// The app's destinations, or one standing for its home when the shell has none.
    public var destinations: [Navree.NavItem] {
        shell?.destinations ?? []
    }

    public var home: String {
        shell?.home ?? "/"
    }
}

/// What happened.
public enum Event: Sendable {
    /// The app launched.
    case start
    /// The person chose a destination.
    case select(String)
    /// The person did something on a drawn screen.
    case intent(Intent, at: PageRef)
    /// The system changed a stack: the back button, the back swipe, a link
    /// that pushes.
    case path(Container, [Step])
    /// The system put the sheet or the cover away (a swipe down).
    case dismissed(Container)
    /// An answer from the origin.
    case answered(Ticket, OriginReply)
    /// A link from outside the app: a universal link, the app's scheme, a
    /// notification.
    case deepLink(URL)
    /// The web page shown at a screen gave itself a title.
    case webTitle(String, at: PageRef)
    /// A link was tapped in the web page shown at a screen.
    case webLink(String, at: PageRef)
    /// The system's sign-in session ended, at its callback or with nothing.
    case signedIn(URL?)
}

/// What the runtime asks the client to do.
public enum Effect: Sendable, Equatable {
    /// Send a request to the origin; its answer comes back as `answered`.
    case fetch(Ticket, OriginRequest)
    /// Open an address outside the app in the system browser (or its app).
    case openExternal(URL)
    /// Start the system's sign-in session at this address on the origin.
    case signIn(URL)
    /// Say something to VoiceOver.
    case announce(String)
    case haptic(Haptic)
    /// Put text on the clipboard.
    case copy(String)
}

/// A request's number and what its answer is for, so an answer that comes
/// after its screen has gone is passed over.
public struct Ticket: Hashable, Sendable {
    public var number: Int
    public var purpose: Purpose
}

public enum Purpose: Hashable, Sendable {
    case shell
    /// A whole screen.
    case page(PageRef, address: String)
    /// A form or a button's request: ops, or a screen in place of this one.
    case action(PageRef, address: String)
    /// A search or filter: the answer's region takes the place of the screen's.
    case query(PageRef, address: String, region: String?)
    /// A region that came empty, or was invalidated.
    case region(PageRef, address: String, id: String)
    /// The records after a list's or table's last.
    case more(PageRef, address: String, target: String)
    /// The end of the sign-in session.
    case signIn

    var ref: PageRef? {
        switch self {
        case .page(let ref, _), .action(let ref, _), .query(let ref, _, _), .region(let ref, _, _), .more(let ref, _, _): ref
        case .shell, .signIn: nil
        }
    }

    /// The address of the screen the answer is for, as it was when asked.
    var pageAddress: String? {
        switch self {
        case .page(_, let address), .action(_, let address), .query(_, let address, _), .region(_, let address, _),
            .more(_, let address, _):
            address
        case .shell, .signIn: nil
        }
    }
}
