/// The kinds of part, and the controls (`input:` and the input's kind), this
/// client does not draw yet. The runtime core sends a page that holds one to
/// the web view, whole, so no screen is ever drawn with a hole in it.
///
/// One word a line, in alphabetical order: when a kind is drawn, its line goes.
public let undrawable: [String] = [
    "web",
]
