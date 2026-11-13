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

    /// A state with a meaning has the symbol of its tone beside its words; a
    /// plain one (a name, a label) is its words alone.
    public static func hasSymbol(_ badge: Navree.Badge) -> Bool {
        switch badge.tone {
        case .success?, .warning?, .danger?, .info?: true
        case .accent?, .neutral?, nil: false
        }
    }
}
