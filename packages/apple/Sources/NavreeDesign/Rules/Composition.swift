import Foundation
import NavreeProtocol
import SwiftUI

/// How a screen written for native clients is set out: the rules of the
/// composition kinds (`card`, `layers`, `shape`, `carousel`, `adaptive`, and
/// the newer words of `stack`, `grid`, `text` and `icon`), as plain functions
/// of the tree, the room it is drawn in and the person's settings.
public enum CompositionRules {
    // MARK: A section's foot

    /// A section's footer as it is drawn on a composed screen's ground:
    /// labelled values (an order's subtotal, shipping and total) are rows of
    /// the section, on its surface after its other rows, label leading and
    /// value trailing in the body's type, as Wallet sets an order's sums; only
    /// words stay a note in small type under the surface.
    public static func footer(_ parts: [Navree.Node]) -> (rows: [Navree.Node], note: [Navree.Node]) {
        var rows: [Navree.Node] = []
        var note: [Navree.Node] = []
        for part in parts {
            if case .facts = part { rows.append(part) } else { note.append(part) }
        }
        return (rows, note)
    }

    /// Where a labelled value's value stands: at the trailing end of its
    /// row, its label at the leading edge. On iPhone and iPad the system's
    /// labelled content sets it there itself; a Mac's sets the value just
    /// after its label outside a form, so in a Mac window the row is set as
    /// a grouped form's row is (System Settings: label leading, value
    /// trailing).
    public enum ValuePlace: Sendable, Equatable {
        /// As the system's labelled content sets it.
        case system
        /// At the trailing end, whatever the system's labelled content does.
        case trailing
    }

    public static func valuePlace(bar: ActionBar) -> ValuePlace {
        bar == .window ? .trailing : .system
    }

    // MARK: The screen

    /// Whether a screen is composed rather than a list of sections: it holds
    /// a card or one of the other composition kinds, or a stack that turns,
    /// grows or pads. Such a screen scrolls on the grouped ground with its
    /// cards as surfaces, as the Health app's Summary does; any other screen
    /// is the system's grouped list.
    public static func isComposed(_ body: Navree.Node) -> Bool {
        body.first { node in
            switch node {
            case .card, .layers, .shape, .carousel, .adaptive: true
            case .stack(let stack): stack.wide != nil || stack.grow || stack.pad > 0
            default: false
            }
        } != nil
    }

    /// The margin between a composed screen's content and the window's edges:
    /// the system's own layout margin, 16 points in a narrow window and 20 in
    /// one at least 414 points wide (a large phone, a tablet, a Mac window),
    /// so the content starts where the bar's title and back button do.
    public static func margin(width: Double) -> Double {
        width >= 414 ? 20 : 16
    }

    /// The width a composed screen's content has: the window's, less its margins.
    public static func room(width: Double) -> Double {
        max(width - 2 * margin(width: width), 0)
    }

    /// The width a composed screen's content has when it stops at a readable
    /// width: the window's less its margins, no wider than `readable`.
    public static func room(width: Double, readable: Double?) -> Double {
        readable.map { min(room(width: width), $0) } ?? room(width: width)
    }

    /// The widest a detail's content stands in a Mac window, in points,
    /// centred in the window: 704, as the system's grouped form sets its
    /// sections (SwiftUI's `Form` with the grouped style on macOS 27, System
    /// Settings' look, stops at 704 points in any wider window and centres
    /// them), and as Apple's layout guidance keeps text to a readable width
    /// (the readable content guide: "the maximum optimal width for text",
    /// "always centered inside the view's layout margin"). A destination's
    /// own screen (a dashboard of tiles) keeps the window's width, and so do
    /// iPhone and iPad, whose windows the system already sizes (`nil`).
    public static func readableWidth(bar: ActionBar, depth: ScreenDepth) -> Double? {
        bar == .window && depth != .root ? macReadableWidth : nil
    }

    /// The width a Mac's grouped form stops at, measured on macOS 27.2 at the
    /// default text size.
    public static let macReadableWidth = 704.0

    // MARK: One leading edge

    /// How far a surface's words stand in from its edges: 16 points, as an
    /// inset grouped list sets its rows and a card pads its parts.
    public static let surfaceInset = 16.0

    /// How far the words that stand on a composed screen's ground (a group's
    /// heading, a note under a surface, a timeline with no surface) stand in
    /// from the edge the surfaces start at. On iPhone and iPad nothing: a
    /// heading starts at its card's edge, as Health's Summary sets them. In a
    /// Mac window as far as a surface's words, so a heading, the words on the
    /// ground and the words in every surface share one leading edge, as a
    /// grouped form's headers, rows and notes do (System Settings).
    public static func groundWordsInset(bar: ActionBar) -> Double {
        bar == .window ? surfaceInset : 0
    }

    /// Whether a part standing on the ground is words that take
    /// ``groundWordsInset(bar:)`` as a whole. A part that holds others (a
    /// stack, a grid, a section) leaves it to them; a surface (a card, a
    /// list, an inset section) and a picture (a chart, a ring, a shape) keep
    /// the ground's edge.
    public static func wordsOnGround(_ node: Navree.Node) -> Bool {
        switch node {
        case .text, .rich, .badge, .timeline, .facts, .progress, .button, .link, .menu, .row, .feedRow, .message,
            .figure, .code, .table, .field, .disclosure:
            true
        default:
            false
        }
    }

    /// A room at least this wide is wide: what a tablet, a large window or a
    /// phone on its side gives a screen's content, and a phone held upright
    /// never does. The same line Android draws, so both clients turn at once.
    public static let wideRoom = 600.0

    /// Whether the room a screen's content is drawn in is wide: by its width
    /// where the drawing knows it (a composed screen measures it), else by the
    /// window's width class. Never by the kind of device.
    public static func isWide(room: Double?, width: WidthClass) -> Bool {
        guard let room else { return width == .regular }
        return room >= wideRoom
    }

    // MARK: Stacks

    /// The axis a stack is set along: its `wide` axis in a wide room, its own
    /// axis otherwise.
    public static func axis(_ stack: Navree.Stack, wide: Bool) -> Navree.Axis {
        wide ? stack.wide ?? stack.axis : stack.axis
    }

    /// Which of an adaptive part's rooms is drawn.
    public static func room(_ adaptive: Navree.Adaptive, wide: Bool) -> [Navree.Node] {
        wide ? adaptive.wide : adaptive.compact
    }

    /// Whether a part takes the room left along a line of parts: a stack or
    /// layers that say so, and a grid, a carousel or a chart, which have no
    /// width of their own. A ring, a shape, an icon or layers with a size keep
    /// their own size.
    public static func grows(_ node: Navree.Node) -> Bool {
        switch node {
        case .stack(let stack): stack.grow
        case .layers(let layers): layers.grow
        case .grid, .carousel, .chart: true
        default: false
        }
    }

    /// Whether a column of parts takes the whole width it is given: one that
    /// centres or ends its parts does, so they stand in the middle (or at the
    /// end) of its place and not of its widest part. Only the width: a column
    /// never takes the height of what stands beside it, so a ring or a clock
    /// in a centring stack keeps its own height and two columns side by side
    /// keep theirs.
    public static func fillsWidth(_ stack: Navree.Stack, axis: Navree.Axis, inLine: Bool) -> Bool {
        // A part that grows takes the room left along a line, and in a
        // column the only room there is, its width.
        if stack.grow { return true }
        return axis == .vertical && !inLine && (stack.align == .center || stack.align == .end)
    }

    /// The space between a stack's parts, in points: its steps, or the
    /// system's own when it gives none.
    public static func spacing(_ gap: Int) -> Double? {
        gap == 0 ? nil : points(steps: gap)
    }

    // MARK: Grids

    /// The room between a grid's parts when it gives none, in points.
    public static let gridGap = 12.0

    /// The least width of a grid's column when the grid gives neither a count
    /// nor a least width.
    public static let gridLeast = 160.0

    /// How many columns a grid has in a place this wide, at most. `cols` is
    /// the most, `min` the least a column may be (grown with the person's
    /// text, so larger text gives fewer, wider columns); a grid that gives
    /// only a count draws that many. Never more columns than parts, never
    /// fewer than one. ``wholeColumns(most:cuts:)`` then keeps every word of
    /// the row whole.
    public static func columns(
        width: Double,
        cols: Int?,
        min least: Int?,
        gap: Double,
        textScale: Double = 1,
        parts: Int
    ) -> Int {
        let most = Swift.max(parts, 1)
        let count: Int
        if let least {
            let fits = fit(width, Double(least) * Swift.max(textScale, 1), gap)
            count = cols.map { Swift.min($0, fits) } ?? fits
        } else if let cols, cols > 0 {
            count = cols
        } else {
            count = fit(width, gridLeast * Swift.max(textScale, 1), gap)
        }
        return Swift.max(1, Swift.min(count, most))
    }

    /// The columns a row of tiles keeps: the most it may have, fewer while a
    /// part at that column's width would cut one of the words it keeps whole
    /// (`cuts`). Siblings in a row share one type size: no name is shrunk on
    /// its own to fit, the row gives each more room instead.
    public static func wholeColumns(most: Int, cuts: (Int) -> Bool) -> Int {
        var count = Swift.max(most, 1)
        while count > 1, cuts(count) { count -= 1 }
        return count
    }

    /// How many parts of this width fit in a place this wide, the gaps
    /// between them counted: at least one. A place with no bound (a line that
    /// scrolls, a measure with no width) holds them all.
    static func fit(_ width: Double, _ part: Double, _ gap: Double) -> Int {
        guard part + gap > 0 else { return Int.max }
        let count = ((width + gap) / (part + gap)).rounded(.down)
        guard count.isFinite else { return Int.max }
        return Swift.max(1, count >= Double(Int.max) ? Int.max : Int(count))
    }

    /// The text sizes a row of tiles may be set in: the person's own, at
    /// every size. Words follow the text size the person chose (Apple's
    /// audit counts a smaller size set by the app as Dynamic Type not
    /// followed); a row whose words do not fit reflows into fewer columns.
    public static func rowTypeSizes(_ size: DynamicTypeSize) -> [DynamicTypeSize] {
        [size]
    }

    /// The width of each column of a grid of `count` columns.
    public static func columnWidth(width: Double, count: Int, gap: Double) -> Double {
        let count = Swift.max(count, 1)
        return Swift.max((width - gap * Double(count - 1)) / Double(count), 0)
    }

    /// Whether a part of a grid's row is as high as the row: a card is, as
    /// the tiles of the Fitness and Health summaries are, so a row of tiles
    /// has one top and one foot. A part with no surface (a figure, a link of
    /// a ring and its words) stands at the top of its cell at its own height.
    public static func fillsRow(_ node: Navree.Node) -> Bool {
        if case .card = node { return true }
        return false
    }

    // MARK: Cards and sections

    /// The corner of a card: the system's continuous 16 points.
    public static let cardCorner = 16.0

    /// The space between a card's parts, in points: two steps.
    public static let cardSpacing = 8.0

    /// The padding inside a card: its own steps, or the system's 16 points; a
    /// card that is a tile in a row of three or more keeps room for its words
    /// with 12.
    public static func padding(_ card: Navree.Card, columns: Int? = nil) -> Double {
        if let pad = card.pad { return points(steps: pad) }
        return (columns ?? 1) >= 3 ? 12 : 16
    }

    /// A section that holds exactly one card is a group: its heading stands
    /// on the page's ground as a section's header does, and the card under it
    /// is the surface. No second surface is drawn for the section.
    public static func isGroup(_ section: Navree.Section) -> Bool {
        guard section.children.count == 1, case .card = section.children[0] else { return false }
        return true
    }

    /// Whether a part of a composed screen is drawn on a surface of its own:
    /// a list (its rows on a card, as an inset grouped list sets them) and a
    /// section that is inset or a card. A section of one card is its card's.
    public static func standsOnSurface(_ node: Navree.Node) -> Bool {
        switch node {
        case .list: true
        case .section(let section): !isGroup(section) && (section.style == .inset || section.style == .card)
        default: false
        }
    }

    // MARK: Carousels

    /// How a carousel lays its parts: how many show at once and how wide each
    /// is. With a least width, as many as fit at it (no more than there are
    /// parts), each the same width. When more are waiting, the next one shows
    /// only its surface's edge and never its words (see `carouselPeek`): the
    /// parts shown take the room, less what that edge needs inside it. With
    /// no least width, each part has its own width (`nil`).
    ///
    /// `margin` is how far the run reaches past the room on each side (the
    /// screen's margin, when the run stands on the screen's ground and goes to
    /// the window's edges); `edge` is how far the parts' words stand in from
    /// their own edges (a card's padding; nothing for a part with no surface).
    public static func carousel(
        room: Double, min least: Int?, gap: Double, parts: Int, margin: Double = 0, edge: Double = 0
    ) -> (shown: Int, width: Double?) {
        guard let least else { return (Swift.max(parts, 1), nil) }
        let column = Double(least)
        let fits = fit(room, column, gap)
        if parts <= fits {
            let shown = Swift.max(parts, 1)
            return (shown, columnWidth(width: room, count: shown, gap: gap))
        }
        let peek = carouselPeek(margin: margin, gap: gap, edge: edge)
        let available = room + margin - gap - peek
        let shown = fit(available, column, gap)
        return (shown, columnWidth(width: available, count: shown, gap: gap))
    }

    /// How much of the next part a carousel shows when more are waiting, in
    /// points: no more than its surface's edge, so a card is never cut through
    /// its words. On the screen's ground the parts shown end with the content
    /// above them, and the next one starts in the margin past the gap, as much
    /// of it showing as the margin leaves; a run inside a surface keeps the
    /// edge inside its room. A part with no surface of its own does not peek:
    /// its words start at its edge.
    public static func carouselPeek(margin: Double, gap: Double, edge: Double) -> Double {
        guard edge > 0 else { return 0 }
        return margin > gap ? Swift.min(margin - gap, edge) : edge
    }

    /// How far a carousel's parts keep their words in from their edges: a
    /// card's padding; nothing when any part has no surface of its own.
    public static func surfaceEdge(_ parts: [Navree.Node]) -> Double {
        let edges = parts.map { node -> Double in
            if case .card(let card) = node { return padding(card) }
            return 0
        }
        return edges.min() ?? 0
    }

    /// Whether a carousel scrolls: only when its parts do not all show.
    public static func scrolls(shown: Int, parts: Int) -> Bool {
        parts > shown
    }

    // MARK: Words and symbols

    /// Points from a number of the system's spacing steps.
    public static func points(steps: Int) -> Double {
        Double(steps) * 4
    }

    /// Whether a text is kept on one line, shrinking a little rather than
    /// breaking: one short word or one figure (`1,994`, `Movement`,
    /// `₹14,36,473`) is never cut in two, as Apple's own tiles keep their
    /// numbers whole. A long run with no space (an address, a code) wraps as
    /// any text does rather than shrinking out of reach.
    public static func keepsWhole(_ text: Navree.Text) -> Bool {
        let value = text.value.trimmingCharacters(in: .whitespaces)
        return !value.isEmpty && value.count <= wholeWord && !value.contains(where: \.isWhitespace)
    }

    /// The longest word kept whole on its line.
    public static let wholeWord = 16

    // MARK: Motion

    /// How words show that they changed.
    public enum ChangeLook: Equatable, Sendable {
        /// The figures roll to the new number.
        case rolls
        /// The old words fade into the new.
        case fades
        /// They change at once.
        case none
    }

    /// How a text shows its change: a count rolls, a fade fades; for a person
    /// who asked for less motion a count only fades.
    public static func change(_ change: Navree.Change?, reduceMotion: Bool) -> ChangeLook {
        switch change {
        case .count: reduceMotion ? .fades : .rolls
        case .fade: .fades
        case nil: .none
        }
    }

    /// How a part comes in when an answer adds it to a screen already drawn.
    public enum Arrival: Equatable, Sendable {
        case fades, rises, grows
        /// It is simply there.
        case none
    }

    /// How a card or a stack comes in: as it says, or only fading for a person
    /// who asked for less motion. A part that says nothing is simply there.
    public static func arrival(_ node: Navree.Node, reduceMotion: Bool) -> Arrival {
        let appear: Navree.Appear?
        switch node {
        case .card(let card): appear = card.appear
        case .stack(let stack): appear = stack.appear
        default: appear = nil
        }
        switch appear {
        case nil: return .none
        case .fade: return .fades
        case .rise: return reduceMotion ? .fades : .rises
        case .scale: return reduceMotion ? .fades : .grows
        }
    }

    /// The identities of parts drawn one after another: a part's own id, or
    /// its place among them, made unique. A part an answer adds is a new
    /// identity, so it comes in as it says; the parts already drawn keep
    /// theirs, and what they hold (a chosen tab, a scroll) stays.
    public static func identities(_ nodes: [Navree.Node]) -> [String] {
        var seen = Set<String>()
        return nodes.enumerated().map { index, node in
            var identity = node.id.map { "#" + $0 } ?? "@\(index)"
            while seen.contains(identity) { identity += "'" }
            seen.insert(identity)
            return identity
        }
    }
}

/// The card that grows into the screen its tap opens, and the screen that
/// shrinks back into it.
public enum ZoomRules {
    /// The id a card is matched by when it grows into the screen its tap
    /// opens: a card that says `zoom`, has an id and navigates. `nil` for any
    /// other card, and for every card when the person asked for less motion
    /// (the screen then comes in as the system brings any screen).
    public static func source(_ card: Navree.Card, reduceMotion: Bool = false) -> String? {
        guard !reduceMotion, card.transition == .zoom, let id = card.id, case .navigate = card.action else { return nil }
        return id
    }

    /// The source a screen the runtime pushed or presented grows out of: the
    /// part it was opened from (`screens[n].from`), when the screen under it
    /// holds a card by that id that is a source. `nil` when it was opened
    /// from anything else (a row, a link, a button) or from nothing.
    public static func source(from: String?, under: Navree.Node?, reduceMotion: Bool = false) -> String? {
        guard let from, let under, case .card(let card)? = under.find(from, kinds: ["card"]) else { return nil }
        return source(card, reduceMotion: reduceMotion) == from ? from : nil
    }

    /// The same, when the screen under it is one of `screens` (the bodies of
    /// the other screens the runtime holds): the first that holds the card.
    public static func source(from: String?, among screens: [Navree.Node], reduceMotion: Bool = false) -> String? {
        guard from != nil else { return nil }
        for under in screens {
            if let source = source(from: from, under: under, reduceMotion: reduceMotion) { return source }
        }
        return nil
    }
}

/// How far a small control's target reaches round what it draws.
public enum HitRules {
    /// The least side of a target Apple asks for on iPhone and iPad (HIG,
    /// Accessibility and Buttons: 44 by 44 points).
    public static let least = 44.0

    /// The smallest an info button's symbol is drawn: a footnote's line.
    public static let smallestGlyph = 16.0

    /// What the target reaches past each side of a symbol `glyph` points
    /// wide, so the whole is at least ``least``: nothing for a symbol that
    /// is that big already.
    public static func outset(glyph: Double) -> Double {
        max(0, ((least - glyph) / 2).rounded(.up))
    }
}

/// More about a part, on request.
public enum HintRules {
    /// The name of the control, or of the VoiceOver action, that opens a
    /// hint: `About the health score`.
    public static func actionName(_ hint: Navree.Hint) -> String {
        let about = hint.about.trimmingCharacters(in: .whitespaces)
        return about.isEmpty ? "About" : "About \(about)"
    }
}
