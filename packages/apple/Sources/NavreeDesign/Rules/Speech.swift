import Foundation
import NavreeProtocol

/// What VoiceOver says of a part, as Apple's own apps have it said: one stop
/// for one thing (a label and its value, a row and what it holds), each word
/// said once, a state said in words and never as a selection, and what a
/// thing offers beyond its words as its actions.
public enum SpeechRules {
    /// A label and its value, said as one stop: `Subtotal, ₹428`, as
    /// Settings says a row's name and its value.
    public static func fact(label: String, value: String) -> String {
        let value = value.trimmingCharacters(in: .whitespaces)
        return value.isEmpty ? label : "\(label), \(value)"
    }

    /// The value a part says after its name, without its name again: a ring
    /// named `Health score` whose summary is `Health score: 79 of 100. Good.`
    /// says `79 of 100. Good.` after it.
    public static func value(_ value: String, after label: String?) -> String {
        guard let label = label?.trimmingCharacters(in: .whitespaces), !label.isEmpty else { return value }
        return dropping(label, from: value) ?? value
    }

    /// `text` with `lead` and what joins it to the rest (`:` `,` `.` `-` and
    /// spaces) taken off its start, ignoring case; `nil` when it does not
    /// start so, or when nothing would be left.
    static func dropping(_ lead: String, from text: String) -> String? {
        guard text.lowercased().hasPrefix(lead.lowercased()) else { return nil }
        let joins = CharacterSet(charactersIn: ":,.-–— ")
        var rest = Substring(text.dropFirst(lead.count))
        // Only a word's end counts: "Recovery" does not lead "Recoveryish".
        if let first = rest.unicodeScalars.first, !joins.contains(first) { return nil }
        while let first = rest.unicodeScalars.first, joins.contains(first) { rest = rest.dropFirst() }
        return rest.isEmpty ? nil : String(rest)
    }

    /// What a bar in a row's detail says, once the row has said its name and
    /// state: its label with the row's title and its state's word taken off
    /// its start (`HRV: Balanced, 58 of 100` in the row `HRV`, `Balanced`
    /// says `58 of 100`), never the percentage the system would add.
    public static func barInRow(_ label: String?, title: String, state: String?) -> String? {
        guard var words = label?.trimmingCharacters(in: .whitespaces), !words.isEmpty else { return nil }
        if let rest = dropping(title, from: words) { words = rest }
        if let state, let rest = dropping(state, from: words) { words = rest }
        return words
    }

    /// Whether a row's trailing words are said: not when its bar has said
    /// them already (`75` after `75 of 100`, `80 steps` after `80 steps`,
    /// `33 / 40` after `33 of 40 points`).
    public static func saysTrailing(_ trailing: String, bar: String?) -> Bool {
        let end = tokens(trailing)
        guard let bar, !end.isEmpty else { return true }
        let said = tokens(bar)
        if said.count >= end.count {
            for start in 0...(said.count - end.count) where Array(said[start..<start + end.count]) == end {
                return false
            }
        }
        // Numbers alone, said in the same order by the bar.
        guard end.allSatisfy(isNumber) else { return true }
        var rest = said[...]
        for number in end {
            guard let found = rest.firstIndex(of: number) else { return true }
            rest = rest[(found + 1)...]
        }
        return false
    }

    static func isNumber(_ token: String) -> Bool {
        !token.isEmpty && token.allSatisfy { $0.isNumber || $0 == "," || $0 == "." }
    }

    /// Words as VoiceOver hears them apart: split at spaces and at marks
    /// other than those inside a number (`2,137`, `7.5`), in lower case.
    static func tokens(_ text: String) -> [String] {
        text.lowercased().split { character in
            character.isWhitespace || (character.isPunctuation && character != "," && character != ".")
        }
        .map { $0.trimmingCharacters(in: CharacterSet(charactersIn: ",.")) }
        .filter { !$0.isEmpty }
    }

    /// Whether a person's picture is said: not when the words beside it name
    /// them already (their whole name, or the first word of it), so a row of
    /// `Tejinder Randhawa` led by his picture says his name once.
    public static func saysAvatar(_ name: String, beside words: [String]) -> Bool {
        let name = name.trimmingCharacters(in: .whitespaces).lowercased()
        let first = name.split(separator: " ").first.map(String.init) ?? name
        return !words.contains { word in
            let word = word.trimmingCharacters(in: .whitespaces).lowercased()
            return word == name || word == first
        }
    }

    /// The person a stack of their picture and words names, when that is all
    /// it holds: said once, by the picture's whole name (`Neha Kapoor`, not
    /// `Neha Kapoor, Neha`).
    public static func person(in stack: Navree.Stack) -> String? {
        guard stack.axis == .horizontal, stack.children.count == 2,
            case .avatar(let avatar) = stack.children[0], case .text(let text) = stack.children[1],
            !saysAvatar(avatar.name, beside: [text.value])
        else { return nil }
        return avatar.name
    }

    /// The words a row's done, current or coming step says, where its symbol
    /// alone shows it.
    public static func step(_ state: Navree.RowState) -> String {
        switch state {
        case .done: "Done"
        case .current: "Current"
        case .upcoming: "Upcoming"
        }
    }

    /// What a row's detail offers beyond its words: each disclosure that
    /// holds a form is one of the row's actions, named by its label (a board
    /// card's `Move`), its form opened in a sheet; a button that only says
    /// the row's own title again is the row's default action, not words.
    public struct RowOffers: Sendable, Equatable {
        public struct Opening: Sendable, Equatable {
            public var label: String
            public var form: Navree.Form
            public var submit: Navree.Button
        }

        public var openings: [Opening] = []
        /// The button that says the row's title again, and the form it posts.
        public var titleButton: Navree.Button?
        public var titleForm: Navree.Form?

        public init() {}
    }

    public static func offers(_ row: Navree.Row) -> RowOffers {
        var offers = RowOffers()
        for part in row.detail {
            part.walk { node in
                switch node {
                case .disclosure(let disclosure):
                    var form: Navree.Form?
                    for child in disclosure.children { child.walk { if case .form(let found) = $0, form == nil { form = found } } }
                    if let form, let submit = submit(of: form) {
                        offers.openings.append(.init(label: disclosure.label, form: form, submit: submit))
                    }
                case .form(let form) where offers.titleButton == nil:
                    if formOnlyRepeats(form, title: row.title), let button = submit(of: form) {
                        offers.titleButton = button
                        offers.titleForm = form
                    }
                case .button(let button) where offers.titleButton == nil && same(button.label, row.title):
                    offers.titleButton = button
                default:
                    break
                }
            }
        }
        return offers
    }

    /// Whether a form holds nothing but one button saying a row's title again.
    public static func formOnlyRepeats(_ form: Navree.Form, title: String) -> Bool {
        var buttons: [Navree.Button] = []
        var fields = 0
        form.walkFields { inner in
            if case .button(let button) = inner { buttons.append(button) }
            if case .field = inner { fields += 1 }
        }
        return fields == 0 && buttons.count == 1 && same(buttons[0].label, title)
    }

    /// Whether a button in a row's detail only says the row's title again.
    public static func repeatsTitle(_ button: Navree.Button, of title: String) -> Bool {
        same(button.label, title)
    }

    static func submit(of form: Navree.Form) -> Navree.Button? {
        var found: Navree.Button?
        form.walkFields { inner in
            if case .button(let button) = inner, found == nil { found = button }
        }
        return found
    }

    static func same(_ a: String, _ b: String) -> Bool {
        a.trimmingCharacters(in: .whitespaces).caseInsensitiveCompare(b.trimmingCharacters(in: .whitespaces)) == .orderedSame
    }
}
