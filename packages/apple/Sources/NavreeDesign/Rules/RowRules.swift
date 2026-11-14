import NavreeProtocol

public enum RowRules {
    /// The lines of a row's detail: states that follow one another share a
    /// line; anything else stands on a line of its own.
    public static func detailLines(_ detail: [Navree.Node]) -> [[Navree.Node]] {
        var lines: [[Navree.Node]] = []
        for part in detail {
            if case .badge = part, let last = lines.last, case .badge? = last.last {
                lines[lines.count - 1].append(part)
            } else {
                lines.append([part])
            }
        }
        return lines
    }

    /// A row's detail without the button that only says the row's title again
    /// (a board card's peek: a form whose one button is the card's title, or
    /// that button alone): the row is that control, opened by a tap or a
    /// click anywhere on it and by VoiceOver's default action
    /// (``SpeechRules/offers(_:)``), so its title is not drawn a second time.
    public static func withoutTitleButton(_ detail: [Navree.Node], title: String) -> [Navree.Node] {
        drawnDetail(detail, title: title, bar: .navigation)
    }

    /// What a row draws of its detail: never the button repeating its title
    /// (``withoutTitleButton(_:title:)``), and on a Mac not a disclosure that
    /// holds a form either (a board card's Move), which the row draws as its
    /// command instead (``rowFormDisclosure(bar:)``).
    public static func drawnDetail(_ detail: [Navree.Node], title: String, bar: ActionBar) -> [Navree.Node] {
        detail.compactMap { part in strip(part, title: title, disclosures: rowFormDisclosure(bar: bar) == .command) }
    }

    private static func strip(_ node: Navree.Node, title: String, disclosures: Bool) -> Navree.Node? {
        switch node {
        case .button(let button) where SpeechRules.repeatsTitle(button, of: title):
            return nil
        case .form(let form) where SpeechRules.formOnlyRepeats(form, title: title):
            return nil
        case .disclosure(let disclosure) where disclosures && holdsForm(disclosure):
            return nil
        case .stack(var stack):
            stack.children = stack.children.compactMap { strip($0, title: title, disclosures: disclosures) }
            return stack.children.isEmpty ? nil : .stack(stack)
        default:
            return node
        }
    }

    private static func holdsForm(_ disclosure: Navree.Disclosure) -> Bool {
        var found = false
        for child in disclosure.children { child.walk { if case .form = $0 { found = true } } }
        return found
    }

    /// Where a disclosure that holds a form in a row (a board card's Move)
    /// stands. On a Mac it is one of the row's commands: the Mac's borderless
    /// button in the accent at the row's foot ("Move…"), the same command in
    /// the row's context menu (a secondary click), each opening its form in a
    /// sheet, and an action VoiceOver offers; never grey words that read as
    /// neither. On iPhone and iPad it stays in the row, a disclosure the
    /// finger opens, and is offered to VoiceOver too.
    public enum FormDisclosure: Sendable, Equatable {
        case inline
        case command
    }

    public static func rowFormDisclosure(bar: ActionBar) -> FormDisclosure {
        bar == .window ? .command : .inline
    }

    /// How a row whose detail holds a button saying its title again opens
    /// what that button opens: from the whole row, by a tap on iPhone and
    /// iPad and a click on a Mac, the button itself not drawn.
    public enum RowOpening: Sendable, Equatable {
        case tap
        case click
    }

    public static func rowOpening(bar: ActionBar) -> RowOpening {
        bar == .window ? .click : .tap
    }

    /// A state with a meaning has the symbol of its tone beside its words; a
    /// plain one (a name, a label) is its words alone.
    public static func hasSymbol(_ badge: Navree.Badge) -> Bool {
        switch badge.tone {
        case .success?, .warning?, .danger?, .info?: true
        case .accent?, .neutral?, nil: false
        }
    }
}
