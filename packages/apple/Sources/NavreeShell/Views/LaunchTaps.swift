import NavreeDesign
import NavreeProtocol

/// A control of a screen found by its words, as a person reading the screen
/// would find it, and what tapping it tells the core: for a demo's launch
/// and its pictures, so a sheet, a question or a pushed screen can be shown
/// as the person would open it.
public enum LaunchTaps {
    public struct Tap: Equatable {
        public var action: Navree.Action
        public var place: Place
    }

    /// The control that says `label`: in an open sheet, dialog or popover
    /// first, then the screen's own actions, then its parts; a row by its
    /// title, a record by its name, a button, link or toolbar item by its words.
    public static func find(_ label: String, in screen: Navree.Screen, open: Set<String> = []) -> Tap? {
        var overlays: [(Navree.Node, String)] = []
        screen.body.walk { node in
            switch node {
            case .sheet(let sheet) where sheet.open || open.contains(sheet.id): overlays.append((node, sheet.id))
            case .dialog(let dialog) where dialog.open || open.contains(dialog.id):
                overlays.append((node, dialog.id))
            case .popover(let popover) where open.contains(popover.id): overlays.append((node, popover.id))
            default: break
            }
        }
        for (node, id) in overlays.reversed() {
            if let tap = search(label, node, form: nil, overlay: id) { return tap }
        }
        for node in ActionRules.expand(screen.actions) {
            if let tap = search(label, node, form: nil, overlay: nil) { return tap }
        }
        return search(label, screen.body, form: nil, overlay: nil, skipOverlays: true)
    }

    static func search(
        _ label: String, _ node: Navree.Node, form: Navree.Form?, overlay: String?, skipOverlays: Bool = false
    ) -> Tap? {
        let place = Place(form: form.map(FormRef.init), overlay: overlay, from: node.id)
        switch node {
        case .sheet, .dialog, .popover:
            if skipOverlays { return nil }
        case .button(let button) where button.label == label:
            return button.action.map { Tap(action: $0, place: Place(
                form: form.map(FormRef.init), overlay: overlay, ask: PresentationRules.question(button), from: button.id
            )) }
        case .link(let link) where link.label == label:
            return Tap(action: link.action, place: place)
        case .row(let row) where row.title == label:
            if let action = row.action { return Tap(action: action, place: place) }
        case .feedRow(let row) where row.runs.map(\.text).joined() == label:
            if let action = row.action { return Tap(action: action, place: place) }
        case .table(let table):
            for record in table.rows {
                let row = RecordRules.row(columns: table.columns, record: record)
                if row.title == label, let action = row.action { return Tap(action: action, place: Place(overlay: overlay)) }
            }
        default:
            break
        }
        let inner: Navree.Form? = if case .form(let found) = node { found } else { form }
        for child in node.children {
            if let tap = search(label, child, form: inner, overlay: overlay, skipOverlays: skipOverlays) { return tap }
        }
        return nil
    }
}
