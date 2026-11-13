import NavreeProtocol

/// What a table's query form offers, read from what each field says it is
/// for (`purpose`), never from what it looks like.
public struct QueryFields: Sendable, Equatable {
    /// The field the search box stands for.
    public var search: Navree.Field?
    /// The fields of the filters sheet.
    public var filters: [Navree.Field]
}

public enum QueryRules {
    /// The search box is the field whose purpose is search (or, from a server
    /// that says no purpose, the text field of the search kind); a field for
    /// the size of a page is left out, since lists here load by cursor; every
    /// other field is a filter.
    public static func fields(_ query: Navree.Form) -> QueryFields {
        var found = QueryFields(search: nil, filters: [])
        query.walkFields { node in
            guard case .field(let field) = node else { return }
            switch field.purpose {
            case .search?:
                if found.search == nil { found.search = field }
            case .pageSize?:
                break
            case .filter?, .sort?:
                found.filters.append(field)
            case nil:
                if case .text(let text) = field.input, text.kind == .search, found.search == nil {
                    found.search = field
                } else {
                    found.filters.append(field)
                }
            }
        }
        return found
    }
}
