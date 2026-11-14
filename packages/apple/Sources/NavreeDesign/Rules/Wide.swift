import Foundation
import NavreeProtocol

/// How wide the window is, in the classes the runtime core is told: what the
/// drawing lays a screen out by, beside what each part is. A narrow window on
/// a tablet (a narrow Stage Manager window, Slide Over) is as compact as a
/// phone held upright, and draws as one.
public enum WindowClass: String, Sendable, Hashable, CaseIterable {
    /// A phone held upright, a narrow window.
    case compact
    /// A tablet held upright, a window of middling width.
    case medium
    /// A tablet held across, a wide window.
    case expanded

    /// The size class the system's own components follow at this width.
    public var widthClass: WidthClass {
        self == .compact ? .compact : .regular
    }

    /// The window's class, from the size class the system gives it and its
    /// width in points: compact as the system's compact size class, and for
    /// any window narrower than a phone held across (a narrow Stage Manager
    /// window), whatever the system says; a regular one is medium until it
    /// is as wide as a tablet held across. A window the system gives no size
    /// class (a Mac's) is never compact: its sidebar folds away by itself,
    /// and a Mac app never takes a phone's tab bar.
    public init(sizeClass: WidthClass?, points: Double) {
        guard let sizeClass else {
            self = points < 1000 ? .medium : .expanded
            return
        }
        if sizeClass == .compact || points < 600 {
            self = .compact
        } else {
            self = points < 1000 ? .medium : .expanded
        }
    }
}

/// A screen that is a list beside what was chosen in it, read from the
/// server's panes: the list, the detail, and the inspector beside the detail.
public struct SplitPanes: Sendable, Equatable {
    /// The list a reader chooses from.
    public var list: Navree.Pane
    /// What was chosen in the list.
    public var detail: Navree.Pane?
    /// The details of what the detail shows, to read or change.
    public var inspector: Navree.Pane?

    public init(list: Navree.Pane, detail: Navree.Pane? = nil, inspector: Navree.Pane? = nil) {
        self.list = list
        self.detail = detail
        self.inspector = inspector
    }
}

/// Which part of a screen of panes a column draws.
public enum SplitPart: Sendable, Hashable {
    case list
    case detail
    case inspector
}

/// Which column a window too narrow for both shows.
public enum CompactColumn: Sendable, Equatable {
    case list
    case detail
}

/// How a destination's stack is drawn.
public enum StackLayout: Sendable, Equatable {
    /// One screen over another, the system's navigation stack.
    case stack
    /// The first screen's list in a column beside its detail, from the
    /// server's panes; screens pushed from the detail stack in its column.
    case panes(SplitPanes)
    /// The first screen (a list whose rows open screens) in a column beside
    /// the screen a row opened, and what is pushed from that in its column.
    case listBesideDetail
}

/// Where a list stands beside its detail, as the system's split view: when
/// the server's panes say so, or when a screen's rows each open a screen of
/// their own. Decided by what the parts are and by the window's width, never
/// by the kind of device.
public enum SplitRules {
    /// How a destination's stack is drawn, from what its first screen holds
    /// and how wide the window is.
    ///
    /// A screen whose panes are led by a list is drawn as columns at every
    /// width: the system's split view folds them into one stack on a compact
    /// width, showing the pane the server says is current. A screen that is
    /// one list whose rows push screens stands beside the screen a row opened
    /// only where there is room: on a compact width its rows push as ever.
    public static func layout(_ body: Navree.Node?, width: WindowClass) -> StackLayout {
        guard let body else { return .stack }
        if let panes = panes(body) { return .panes(panes) }
        if width != .compact, rowsOpenScreens(body) { return .listBesideDetail }
        return .stack
    }

    /// The panes of a body that is a split led by a list pane: the list, the
    /// first pane after it as the detail, and an inspector pane beside that.
    /// Any other split (content beside what supports it) stands inside its
    /// screen, not as columns.
    public static func panes(_ body: Navree.Node) -> SplitPanes? {
        guard case .split(let split) = sole(body), case .pane(let list) = split.primary, list.role == .list else {
            return nil
        }
        let rest = flattened(split.detail)
        guard !rest.isEmpty, rest.count == countOfParts(split.detail) else { return nil }
        var found = SplitPanes(list: list)
        found.inspector = rest.last { $0.role == .inspector }
        found.detail = rest.first { $0.role != .inspector }
        return found
    }

    /// Which pane a window too narrow for both columns shows first: the one
    /// the server marks current; the list when none is.
    public static func compactColumn(_ panes: SplitPanes) -> CompactColumn {
        if panes.list.current { return .list }
        if panes.detail?.current == true || panes.inspector?.current == true { return .detail }
        return .list
    }

    /// Whether the inspector stands beside the detail as the screen opens: only
    /// where the window is wide enough for three columns. Elsewhere it waits
    /// for the person to ask, and a compact window shows it as a sheet.
    public static func showsInspector(width: WindowClass) -> Bool {
        width == .expanded
    }

    /// The least width an inspector column takes beside its detail.
    public static let inspectorLeast = 280.0

    /// The room a window of columns leaves its detail: its width less the
    /// sidebar's (220) and the list's (360) columns at their ideal widths.
    public static func detailRoom(window: Double, sidebar: Bool) -> Double {
        window - (sidebar ? 220 : 0) - 360
    }

    /// Whether the inspector fits in the room the detail's column has (the
    /// window less its sidebar and its list): the detail keeps its least
    /// width beside it (``NavigationRules/detailMinimumWidth``), or the
    /// inspector waits behind its bar button, rather than the two hanging
    /// over the list's column and out of the window. A column not measured
    /// yet says nothing against it.
    public static func inspectorFits(detailRoom: Double?) -> Bool {
        guard let detailRoom else { return true }
        return detailRoom >= NavigationRules.detailMinimumWidth + inspectorLeast
    }

    /// Whether the inspector is a sheet the person opens rather than a
    /// column: wherever it does not stand beside the detail from the start
    /// (``showsInspector(width:)``), as in a compact window, where the
    /// system's inspector is a sheet too. The sheet is there only while it is
    /// open, so nothing of it waits out of sight in the window: kept out of
    /// sight as a column, the system's inspector put its title ("Customer")
    /// in the detail's bar about one launch in six on iPhone, and on an iPad
    /// held upright pushed the columns' bars 54 points below the tab bar on a
    /// first launch.
    /// On a Mac, where a window's inspector is a column the person shows
    /// and hides, it stays a column at every width.
    public static func inspectorIsSheet(width: WindowClass, bar: ActionBar = .navigation) -> Bool {
        bar == .navigation && !showsInspector(width: width)
    }

    /// The destinations whose first screen is drawn as columns, as far as
    /// their first screens are known: what a tab view with its bar at the top
    /// is built anew for when it changes. Laid out round a stack (a screen
    /// still on its way), the tab view keeps its bar in the bar's row when a
    /// split view takes the stack's place, and the columns' own bars run under
    /// it, a list's title cut by the tabs; built round the split view, the
    /// tab bar stands in a row of its own above the columns' bars.
    public static func columnDestinations(_ layouts: [StackLayout]) -> [Int] {
        layouts.indices.filter { layouts[$0] != .stack }
    }

    /// Whether a split's columns stand side by side, or fold into one stack.
    public static func sideBySide(width: WindowClass) -> Bool {
        width != .compact
    }

    /// The screen one column draws: the pane's parts as its body and the
    /// pane's title, with the screen's actions that belong there. The list
    /// keeps the screen's title, its search and its actions that make
    /// something new (Mail's compose stands over its list); the detail takes
    /// every other action and the sheets and dialogs of the screen.
    public static func screen(_ part: SplitPart, of screen: Navree.Screen, panes: SplitPanes) -> Navree.Screen {
        let overlays = screen.body.children.filter(isOverlay)
        switch part {
        case .list:
            return Navree.Screen(
                actions: screen.actions.filter(belongsToList), back: screen.back,
                body: .stack(Navree.Stack(children: panes.list.children, id: panes.list.id)),
                id: screen.id, nav: screen.nav, refresh: screen.refresh, search: screen.search,
                subtitle: screen.subtitle, title: panes.list.title ?? screen.title, trail: screen.trail
            )
        case .detail:
            let pane = panes.detail
            return Navree.Screen(
                actions: screen.actions.filter { !belongsToList($0) },
                body: .stack(Navree.Stack(children: (pane?.children ?? []) + overlays, id: pane?.id)),
                id: screen.id, nav: screen.nav, pager: screen.pager, refresh: screen.refresh,
                title: pane?.title ?? ""
            )
        case .inspector:
            let pane = panes.inspector
            return Navree.Screen(
                body: .stack(Navree.Stack(children: pane?.children ?? [], id: pane?.id)),
                id: screen.id, nav: screen.nav, title: pane?.title ?? ""
            )
        }
    }

    /// The screen a column draws when it draws one part of a screen of panes;
    /// the screen itself when it draws all of it, or when it holds no panes.
    public static func part(_ part: SplitPart?, of screen: Navree.Screen) -> Navree.Screen {
        guard let part, let panes = panes(screen.body) else { return screen }
        return self.screen(part, of: screen, panes: panes)
    }

    /// What a choice in the list of a screen of panes asks for: the screen of
    /// panes for that row, in place of this one. A choice is not a step
    /// deeper, so it never stacks one split on another; anything that is not
    /// a link to a screen goes as it came.
    public static func choice(_ action: Navree.Action) -> Navree.Action {
        guard case .navigate(var link) = action, link.present == .auto || link.present == .push else { return action }
        link.present = .replace
        return .navigate(link)
    }

    /// Whether a body is one list or table whose rows each open a screen
    /// pushed on it: a list of mail, of orders, of notes. Rows that open a
    /// sheet, act in place or open nothing leave the screen as it is.
    public static func rowsOpenScreens(_ body: Navree.Node) -> Bool {
        guard ContentRules.list(body) == .plain else { return false }
        let actions: [Navree.Action?]
        switch sole(body) {
        case .list(let list):
            actions = list.children.compactMap { node -> Navree.Action?? in
                if case .row(let row) = node { return .some(row.action) }
                return nil
            }
        case .table(let table):
            actions = table.rows.map(\.action)
        default:
            return false
        }
        return !actions.isEmpty && actions.allSatisfy { action in
            guard case .navigate(let link)? = action else { return false }
            return link.present == .auto || link.present == .push
        }
    }

    /// A split that stands among a screen's other parts: its two sides beside
    /// each other where the window is wider than compact; on a compact one,
    /// one over the other with the side the server marks current first.
    public static func inPlace(_ split: Navree.Split, width: WindowClass) -> (sideBySide: Bool, panes: [Navree.Node]) {
        let sides = [split.primary, split.detail]
        if width != .compact { return (true, sides) }
        func current(_ node: Navree.Node) -> Bool {
            if case .pane(let pane) = node { return pane.current }
            return false
        }
        return (false, sides.filter(current) + sides.filter { !current($0) })
    }

    /// The row of a list beside its detail that opened the detail shown (the
    /// one whose link leads to the detail's address), with the actions of its
    /// context menu: what the menu bar's commands for the chosen thing act on,
    /// as Mail's Message menu acts on the message open.
    public static func chosenRow(in body: Navree.Node, opened address: String) -> (title: String, actions: [Navree.Node])? {
        let opened = path(of: address)
        func leads(_ action: Navree.Action?) -> Bool {
            guard case .navigate(let link)? = action else { return false }
            return path(of: link.href) == opened
        }
        switch sole(body) {
        case .list(let list):
            for node in list.children {
                if case .row(let row) = node, leads(row.action) {
                    return (row.title, row.menu + row.swipe.map(Navree.Node.button))
                }
            }
        case .table(let table):
            for record in table.rows where leads(record.action) {
                let row = RecordRules.row(columns: table.columns, record: record)
                let controls = row.controls.flatMap(RecordRules.controls(in:)).filter { !$0.opensSheet }
                return (row.title, record.actions + controls.map { .button($0.button) })
            }
        default:
            break
        }
        return nil
    }

    /// The id of the row of a list beside its detail that opened the detail
    /// shown: the list's selection, as Mail highlights the message open and no
    /// other. A row ticked for an action on many is no part of it.
    public static func openRow(in body: Navree.Node, opened address: String) -> String? {
        let opened = path(of: address)
        func leads(_ action: Navree.Action?) -> Bool {
            guard case .navigate(let link)? = action else { return false }
            return path(of: link.href) == opened
        }
        var found: String?
        body.walk { node in
            guard found == nil else { return }
            switch node {
            case .row(let row) where leads(row.action): found = row.id
            case .table(let table): found = table.rows.first { leads($0.action) }?.id
            default: break
            }
        }
        return found
    }

    /// An address's path: what is left of it without its origin, query and fragment.
    static func path(of address: String) -> String {
        var rest = Substring(address)
        if let scheme = rest.range(of: "://") {
            let after = rest[scheme.upperBound...]
            rest = after.firstIndex(of: "/").map { after[$0...] } ?? "/"
        }
        let path = rest.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        let clean = path.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        return clean.isEmpty ? "/" : String(clean)
    }

    // MARK: - Reading the tree

    /// The one part a body holds besides its sheets and dialogs, through the
    /// stacks and regions that only wrap it.
    static func sole(_ node: Navree.Node) -> Navree.Node {
        switch node {
        case .stack(let stack) where !stack.hidden:
            let parts = stack.children.filter { !isOverlay($0) }
            return parts.count == 1 ? sole(parts[0]) : node
        case .region(let region) where region.children.count == 1:
            return sole(region.children[0])
        default:
            return node
        }
    }

    /// The panes of a split's detail side, in order.
    static func flattened(_ node: Navree.Node) -> [Navree.Pane] {
        switch node {
        case .pane(let pane): [pane]
        case .split(let split): flattened(split.primary) + flattened(split.detail)
        default: []
        }
    }

    /// How many parts a split's detail side holds, panes or not: a side that
    /// holds anything but panes is not columns.
    static func countOfParts(_ node: Navree.Node) -> Int {
        if case .split(let split) = node { return countOfParts(split.primary) + countOfParts(split.detail) }
        return 1
    }

    static func isOverlay(_ node: Navree.Node) -> Bool {
        switch node {
        case .sheet, .dialog, .popover: true
        default: false
        }
    }

    /// An action that makes something new stands over the list it adds to.
    static func belongsToList(_ node: Navree.Node) -> Bool {
        guard case .button(let button) = node else { return false }
        return button.role == .create || button.standard == .add
    }
}

/// A control in a table's cell, as a wide table draws it: one button, never a
/// form spread across the cell. A form with fields to fill opens them in a
/// sheet with the form's own submit as Done, as a narrow window's swipe action
/// does; a form or button with nothing to fill acts at once.
public struct CellControl: Sendable, Equatable {
    public var button: Navree.Button
    public var form: Navree.Form?
    /// The number of fields the form holds: filled in a sheet the button opens.
    public var fields: Int

    /// The form has fields to fill in a sheet before it is sent.
    public var opensSheet: Bool { form != nil && fields > 0 }
}

extension RecordRules {
    /// The controls a record's cell holds: each form that has a submit, with
    /// its submit and how many fields it holds; else each button.
    public static func controls(in cell: Navree.Node) -> [CellControl] {
        var found: [CellControl] = []
        cell.walk { node in
            guard case .form(let form) = node else { return }
            var submit: Navree.Button?
            var fields = 0
            form.walkFields { inner in
                if case .button(let button) = inner, submit == nil { submit = button }
                if case .field = inner { fields += 1 }
            }
            if let submit { found.append(CellControl(button: submit, form: form, fields: fields)) }
        }
        if found.isEmpty {
            cell.walk { node in
                if case .button(let button) = node { found.append(CellControl(button: button, form: nil, fields: 0)) }
            }
        }
        return found
    }
}

/// How a date or a time stands in a form.
public enum DateStyle: Sendable, Equatable {
    /// The system's compact picker: the value as a button that opens the
    /// calendar or the clock (a field with a calendar on a Mac), as Calendar
    /// and Reminders set a date in a form.
    case compact
}

extension ContentRules {
    /// A date in a form row is the compact picker on every system. The
    /// wheels are the system's own for a picker standing alone, never a
    /// form's row; the calendar laid open is the day pager's.
    public static func dateStyle(_ input: Navree.DateInput) -> DateStyle {
        .compact
    }
}

/// How a pull-down menu in the content shows what it is.
public enum MenuLabel: Sendable, Equatable {
    /// Its symbol and its words.
    case titleAndIcon
    /// Its words alone: it has no symbol.
    case title
    /// Its symbol alone: it has no words to show (VoiceOver still names it).
    case icon
}

extension ContentRules {
    /// A menu in the content shows its words beside its symbol: a menu in a
    /// row of controls often names what it sets ("Open", "High"), as Reminders
    /// writes a reminder's priority, and a symbol alone leaves the person to
    /// guess. A toolbar's menus keep their symbols alone, as bars do.
    public static func menuLabel(_ menu: Navree.Menu) -> MenuLabel {
        let words = !menu.label.trimmingCharacters(in: .whitespaces).isEmpty
        guard menu.icon != nil else { return .title }
        return words ? .titleAndIcon : .icon
    }
}
