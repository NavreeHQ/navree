import NavreeProtocol
import SwiftUI
#if os(iOS)
import UIKit
#endif

// The composition kinds, drawn with the system's own views as Apple's Health
// and Fitness summaries set theirs out: the screen scrolls on the grouped
// ground, cards are its surfaces, a group's heading stands on the ground at
// the same leading edge as everything else, rows of tiles share one top and
// one foot, and runs of cards scroll across edge to edge. Nothing here is
// glass, has a shadow or a border of its own: cards and shapes are content.

// MARK: The screen

/// A screen written for native clients: its parts on the grouped ground,
/// scrolling, with the cards as surfaces.
public struct ComposedScreenBody: View {
    let body_: Navree.Node

    public init(body: Navree.Node) {
        body_ = body
    }

    public var body: some View {
        ScrollView {
            ComposedContent(body: body_)
        }
        .background(Surfaces.ground)
    }
}

/// The parts of a composed screen, set out at the system's margins, without
/// the scroll view that holds them. It measures the room it is given, so a
/// stack that turns and a part for each room follow the room's width (a
/// window made narrower, a phone on its side) and never the kind of device.
public struct ComposedContent: View {
    let node: Navree.Node
    @State private var width: Double?

    public init(body: Navree.Node) {
        node = body
    }

    public var body: some View {
        let margin = width.map(CompositionRules.margin(width:)) ?? 16
        NodeView(node)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, CGFloat(margin))
            .padding(.top, 8)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity)
            .environment(\.navreeRoomWidth, width.map(CompositionRules.room(width:)))
            .environment(\.navreeScreenMargin, CGFloat(margin))
            .environment(\.navreeOnGround, true)
            .onGeometryChange(for: Double.self) { Double($0.size.width) } action: { width = $0 }
            .background(Surfaces.ground)
    }
}

/// Whether the room a part is drawn in is wide: by the composed screen's
/// measured room, else by the window's width class.
struct Room: DynamicProperty {
    @Environment(\.navreeRoomWidth) private var room
    @Environment(\.horizontalSizeClass) private var sizeClass

    var wide: Bool {
        CompositionRules.isWide(room: room, width: WidthClass(sizeClass))
    }
}

// MARK: Parts one after another

/// A part among others, with what it needs to be drawn among them: a stable
/// identity, and the words just above it.
struct PartItem: Identifiable {
    let id: String
    let node: Navree.Node
    let wordsAbove: [String]

    static func of(_ nodes: [Navree.Node]) -> [PartItem] {
        zip(CompositionRules.identities(nodes), nodes.indices).map { identity, index in
            PartItem(id: identity, node: nodes[index], wordsAbove: HeadingRules.wordsAbove(nodes, index: index))
        }
    }
}

/// Parts one after another, each with its identity, so a part an answer adds
/// comes in as it says (`appear`) and the parts already drawn stay as they are.
struct Parts: View {
    let nodes: [Navree.Node]
    /// How a part that grows takes the room left along the line it stands in.
    var growth: Navree.Axis? = nil
    /// Whether the parts are spread along the line, room between them.
    var spread = false

    var body: some View {
        ForEach(Array(PartItem.of(nodes).enumerated()), id: \.element.id) { index, item in
            if spread, index > 0 { Spacer(minLength: 8) }
            NodeView(item.node)
                .frame(maxWidth: growth == .horizontal && CompositionRules.grows(item.node) ? .infinity : nil, alignment: .leading)
                .environment(\.navreeWordsAbove, item.wordsAbove)
                .modifier(Arriving(node: item.node))
        }
    }
}

/// How a part comes in when an answer adds it to a screen already drawn. The
/// container animates the change of its parts with the system's own timing;
/// at a screen's first drawing nothing changes, so nothing moves.
struct Arriving: ViewModifier {
    let node: Navree.Node
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.transition(Self.transition(CompositionRules.arrival(node, reduceMotion: reduceMotion)))
    }

    static func transition(_ arrival: CompositionRules.Arrival) -> AnyTransition {
        switch arrival {
        case .fades: .opacity
        case .rises: .offset(y: 16).combined(with: .opacity)
        case .grows: .scale(scale: 0.92).combined(with: .opacity)
        case .none: .identity
        }
    }
}

extension View {
    /// Animates the arrival and departure of parts among `nodes`.
    func animatesParts(_ nodes: [Navree.Node]) -> some View {
        animation(.default, value: CompositionRules.identities(nodes))
    }
}

// MARK: Cards

/// A card: a surface of content on the grouped ground, its parts one under
/// another. Tapped as a whole, it is one control: one target with the
/// system's pressed state, one element to VoiceOver named by its label, and
/// nothing in it takes a tap of its own. Its menu opens on a long press and a
/// secondary click. A card that zooms grows into the screen it opens.
struct CardPart: View {
    let card: Navree.Card
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay
    @Environment(\.navreeInControl) private var inControl
    @Environment(\.navreeGridColumns) private var columns
    @Environment(\.navreeInTileRow) private var inRow
    @Environment(\.navreeZoomSpace) private var zoomSpace
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorScheme) private var scheme

    private var shape: RoundedRectangle {
        RoundedRectangle(cornerRadius: CompositionRules.cardCorner, style: .continuous)
    }

    var body: some View {
        Group {
            if let action = card.action, !inControl {
                Button {
                    context.send(.act(action, Place(overlay: overlay, from: card.id)))
                } label: {
                    surface.contentShape(.interaction, shape)
                }
                .buttonStyle(.plain)
                .modifier(PointerHover(corner: CompositionRules.cardCorner))
                .environment(\.navreeInControl, true)
                .modifier(OneControl(label: card.label))
            } else {
                surface
            }
        }
        .modifier(CardMenu(items: card.menu, shape: shape))
        .modifier(ZoomSource(id: ZoomRules.source(card, reduceMotion: reduceMotion), space: zoomSpace, ground: Surfaces.card))
    }

    private var surface: some View {
        VStack(alignment: .leading, spacing: CGFloat(CompositionRules.cardSpacing)) {
            Parts(nodes: card.children)
        }
        .animatesParts(card.children)
        .padding(CGFloat(CompositionRules.padding(card, columns: columns)))
        .frame(maxWidth: .infinity, maxHeight: inRow ? .infinity : nil, alignment: .topLeading)
        .background(tint, in: shape)
        .background(Surfaces.card, in: shape)
        .environment(\.navreeOnGround, false)
        .environment(\.navreeScreenMargin, nil)
        .environment(\.navreeInTileRow, false)
        .environment(\.navreeGridColumns, nil)
    }

    /// A hue's soft tint over the card's own ground, or nothing.
    private var tint: Color {
        guard let colour = card.colour else { return .clear }
        return Hues.color(colour, scheme: scheme).opacity(0.15)
    }
}

/// A part tapped as a whole, to VoiceOver: one button, named by its label
/// when it has one, else by the words inside it read together.
struct OneControl: ViewModifier {
    let label: String?

    func body(content: Content) -> some View {
        if let label, !label.isEmpty {
            content
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(label)
                .accessibilityAddTraits(.isButton)
        } else {
            content
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isButton)
        }
    }
}

/// A card's menu, on a long press and a secondary click, with the card itself
/// lifted as its preview. VoiceOver offers it as the element's actions.
struct CardMenu: ViewModifier {
    let items: [Navree.Node]
    let shape: RoundedRectangle

    func body(content: Content) -> some View {
        if items.isEmpty {
            content
        } else {
            #if os(iOS)
            content
                .contentShape(.contextMenuPreview, shape)
                .contextMenu { MenuItems(items: items) }
            #else
            content.contextMenu { MenuItems(items: items) }
            #endif
        }
    }
}

// MARK: Sections on the ground

/// The heading of a group on the page's ground, as Health's Summary sets its
/// own: the title bold, the line under it in the secondary colour, its hint
/// beside the title and its actions at the end. It starts at the same
/// leading edge as the cards and the words around it.
struct GroupHeading: View {
    let section: Navree.Section

    var body: some View {
        if section.title != nil || section.subtitle != nil || !section.actions.isEmpty {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    if section.title != nil || section.hint != nil {
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            if let title = section.title {
                                Text(title).font(.title3.weight(.bold)).accessibilityAddTraits(.isHeader)
                            }
                            if let hint = section.hint { HintButton(hint: hint) }
                        }
                    }
                    if let subtitle = section.subtitle {
                        Text(subtitle).font(.footnote).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 8)
                ForEach(Array(section.actions.enumerated()), id: \.offset) { _, action in
                    NodeView(action).font(.subheadline)
                }
            }
        }
    }
}

/// A section that holds one card: its heading on the ground and the card
/// under it, one group with one surface.
struct GroupPart: View {
    let section: Navree.Section

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupHeading(section: section)
            Parts(nodes: section.children + section.footer)
        }
        .animatesParts(section.children + section.footer)
    }
}

/// Any other section standing on a composed screen's ground: its heading as a
/// group's, its parts under it; a list among them, or the whole of a section
/// that is inset or a card, on one surface as an inset grouped list sets it.
struct GroundSectionPart: View {
    let section: Navree.Section

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupHeading(section: section)
            if section.style == .inset || section.style == .card {
                Surface { Parts(nodes: section.children) }
            } else {
                Parts(nodes: section.children)
            }
            Parts(nodes: section.footer)
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .animatesParts(section.children)
    }
}

/// A surface of content on the ground with no words of its own: what a list
/// or an inset section stands on in a composed screen.
struct Surface<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: CGFloat(CompositionRules.cardSpacing)) {
            content
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Surfaces.card, in: RoundedRectangle(cornerRadius: CompositionRules.cardCorner, style: .continuous))
        .environment(\.navreeOnGround, false)
        .environment(\.navreeScreenMargin, nil)
    }
}

/// Rows one under another with the system's separators between them, on a
/// surface when they stand on a composed screen's ground.
struct ListPart: View {
    let list: Navree.List
    @Environment(\.navreeOnGround) private var onGround

    var body: some View {
        if onGround {
            Surface { rows }
        } else {
            rows
        }
    }

    private var rows: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(PartItem.of(list.children).enumerated()), id: \.element.id) { index, item in
                if index > 0 { Divider() }
                NodeView(item.node).modifier(Arriving(node: item.node))
            }
        }
        .animatesParts(list.children)
    }
}

// MARK: Layers and shapes

/// Parts over each other, the first at the back, all at one place. A shape
/// with no size of its own fills the box: the box the layers are given, or,
/// with none, the box the other parts make.
struct LayersPart: View {
    let layers: Navree.Layers
    @ScaledMetric(relativeTo: .body) private var unit: CGFloat = 1

    var body: some View {
        let factor = layers.size?.scaled == true ? unit : 1
        let width = layers.size?.width.map { CGFloat($0) * factor }
        let height = layers.size?.height.map { CGFloat($0) * factor }
        Group {
            if width != nil || height != nil || !holdsFillers {
                ZStack(alignment: alignment) {
                    Parts(nodes: layers.children)
                }
                .frame(width: width, height: height, alignment: alignment)
            } else {
                // The parts with a size make the box; the shapes with none
                // fill it from behind, in their order.
                ZStack(alignment: alignment) {
                    Parts(nodes: layers.children.filter { !Self.fills($0) })
                }
                .background {
                    ZStack { Parts(nodes: layers.children.filter(Self.fills)) }
                }
            }
        }
        .animatesParts(layers.children)
        .padding(CGFloat(CompositionRules.points(steps: layers.pad)))
    }

    private var holdsFillers: Bool {
        layers.children.contains(where: Self.fills)
    }

    /// A shape with no size fills the box it stands in.
    static func fills(_ node: Navree.Node) -> Bool {
        if case .shape(let shape) = node { return shape.size == nil }
        return false
    }

    private var alignment: Alignment {
        switch layers.place {
        case .topStart: .topLeading
        case .top: .top
        case .topEnd: .topTrailing
        case .start: .leading
        case .center: .center
        case .end: .trailing
        case .bottomStart: .bottomLeading
        case .bottom: .bottom
        case .bottomEnd: .bottomTrailing
        }
    }
}

/// A shape: a rounded rectangle, a circle, a capsule or a rectangle, filled
/// in its hue (the hue's soft tint when it is a ground for what stands on it,
/// a blend along it when it names a second hue), with a hairline in its
/// stroke's hue. Decoration, hidden from VoiceOver, unless it has a label.
struct ShapePart: View {
    let shape: Navree.Shape
    @Environment(\.colorScheme) private var scheme
    @Environment(\.displayScale) private var displayScale
    @ScaledMetric(relativeTo: .body) private var unit: CGFloat = 1

    var body: some View {
        let factor = shape.size?.scaled == true ? unit : 1
        form
            .frame(width: shape.size?.width.map { CGFloat($0) * factor }, height: shape.size?.height.map { CGFloat($0) * factor })
            .accessibilityElement()
            .accessibilityLabel(shape.label ?? "")
            .accessibilityHidden(shape.label == nil)
    }

    private var fill: AnyShapeStyle {
        guard let colour = shape.fill else { return AnyShapeStyle(Color.clear) }
        let start = Hues.color(colour, scheme: scheme)
        let strength = shape.soft ? 0.15 : 1
        if colour.to != nil {
            let end = Hues.end(colour, scheme: scheme)
            return AnyShapeStyle(LinearGradient(colors: [start.opacity(strength), end.opacity(strength)], startPoint: .leading, endPoint: .trailing))
        }
        return AnyShapeStyle(start.opacity(strength))
    }

    private var stroke: Color {
        shape.stroke.map { Hues.color($0, fallback: .secondary, scheme: scheme) } ?? .clear
    }

    private var hairline: CGFloat {
        shape.stroke == nil ? 0 : 1 / max(displayScale, 1)
    }

    @ViewBuilder private var form: some View {
        switch shape.form {
        case .rounded:
            RoundedRectangle(cornerRadius: 12, style: .continuous).fill(fill)
                .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(stroke, lineWidth: hairline))
        case .circle:
            Circle().fill(fill).overlay(Circle().strokeBorder(stroke, lineWidth: hairline))
        case .capsule:
            Capsule().fill(fill).overlay(Capsule().strokeBorder(stroke, lineWidth: hairline))
        case .rect:
            Rectangle().fill(fill).overlay(Rectangle().strokeBorder(stroke, lineWidth: hairline))
        }
    }
}

// MARK: A part for each room

/// One of two rooms: the wide one where the screen's room is wide, the
/// compact one otherwise, changing over as the room does.
struct AdaptivePart: View {
    let adaptive: Navree.Adaptive
    private let room = Room()

    var body: some View {
        let parts = CompositionRules.room(adaptive, wide: room.wide)
        VStack(alignment: .leading, spacing: 12) {
            Parts(nodes: parts)
        }
        .animatesParts(parts)
    }
}

// MARK: Grids

/// Parts in columns: as many as the grid's least width lets fit, no more than
/// its count, each column the same width. A row of cards (tiles) is as high
/// as its tallest, so the row has one top and one foot; other parts stand at
/// the top of their cells. The words a tile keeps whole are never cut and
/// never shrunk one by one: a grid that gives a count keeps it with the whole
/// row's text a size or two smaller where that is all it takes (never at the
/// accessibility sizes), and otherwise draws fewer, wider columns.
struct GridPart: View {
    let grid: Navree.Grid
    private let scale = TextScale()
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let sizes = CompositionRules.rowTypeSizes(typeSize)
        Group {
            if grid.cols == nil {
                tiles(keepsCount: false)
            } else {
                ViewThatFits(in: .horizontal) {
                    tiles(keepsCount: true)
                    if sizes.count > 1 { tiles(keepsCount: true).dynamicTypeSize(sizes[1]) }
                    if sizes.count > 2 { tiles(keepsCount: true).dynamicTypeSize(sizes[2]) }
                    tiles(keepsCount: false)
                }
            }
        }
        .environment(\.navreeGridColumns, grid.cols ?? 2)
        .animatesParts(grid.children)
    }

    private func tiles(keepsCount: Bool) -> some View {
        let gap = grid.gap > 0 ? CompositionRules.points(steps: grid.gap) : CompositionRules.gridGap
        return TileGrid(
            cols: grid.cols, least: grid.min, gap: gap, rowGap: max(gap, CompositionRules.gridGap),
            textScale: Double(scale.capped(4)), keepsCount: keepsCount
        ) {
            ForEach(Array(PartItem.of(grid.children).enumerated()), id: \.element.id) { _, item in
                NodeView(item.node)
                    .environment(\.navreeInTileRow, CompositionRules.fillsRow(item.node))
                    .environment(\.navreeInTiles, true)
                    .layoutValue(key: FillsRow.self, value: CompositionRules.fillsRow(item.node))
                    .modifier(Arriving(node: item.node))
            }
        }
    }
}

/// Whether a cell is as high as its row.
struct FillsRow: LayoutValueKey {
    static let defaultValue = false
}

/// The grid's own layout: its columns from the width it is given, as
/// ``CompositionRules/columns(width:cols:min:gap:textScale:parts:)`` counts
/// them, and rows as high as their tallest cell. A grid that keeps its count
/// asks, when measured with no width, for the room its count needs with
/// every cell at its least (its whole words on their lines); one that does
/// not draws fewer columns where a cell would cut a word.
struct TileGrid: Layout {
    let cols: Int?
    let least: Int?
    let gap: Double
    let rowGap: Double
    let textScale: Double
    var keepsCount = false

    struct Plan {
        var count: Int
        var column: CGFloat
        var rows: [CGFloat]
    }

    /// The least a cell can be: as narrow as it goes, its whole words kept.
    private func cellLeast(_ subviews: Subviews) -> Double {
        subviews.map { Double($0.sizeThatFits(ProposedViewSize(width: 1, height: nil)).width) }.max() ?? 0
    }

    /// The width the grid needs when nothing is offered.
    private func natural(_ subviews: Subviews) -> Double {
        let count = Swift.max(Swift.min(cols ?? 2, subviews.count), 1)
        var column = least.map { Double($0) * Swift.max(textScale, 1) } ?? (cols == nil ? CompositionRules.gridLeast : 0)
        if keepsCount { column = Swift.max(column, cellLeast(subviews)) }
        return column * Double(count) + gap * Double(count - 1)
    }

    private func plan(width: CGFloat?, subviews: Subviews) -> Plan {
        let room: Double
        if let width, width.isFinite {
            room = Double(width)
        } else {
            room = natural(subviews)
        }
        let most = CompositionRules.columns(
            width: room, cols: cols, min: least, gap: gap, textScale: textScale, parts: subviews.count
        )
        // A part too narrow for a word it keeps whole is wider than offered.
        let count = keepsCount ? most : CompositionRules.wholeColumns(most: most) { count in
            let column = CGFloat(CompositionRules.columnWidth(width: room, count: count, gap: gap))
            return subviews.contains { $0.sizeThatFits(ProposedViewSize(width: column, height: nil)).width > column + 0.5 }
        }
        let column = CGFloat(CompositionRules.columnWidth(width: room, count: count, gap: gap))
        var rows: [CGFloat] = []
        var index = 0
        while index < subviews.count {
            let row = subviews[index..<Swift.min(index + count, subviews.count)]
            rows.append(row.map { $0.sizeThatFits(ProposedViewSize(width: column, height: nil)).height }.max() ?? 0)
            index += count
        }
        return Plan(count: count, column: column, rows: rows)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let plan = plan(width: proposal.width, subviews: subviews)
        let height = plan.rows.reduce(0, +) + CGFloat(rowGap) * CGFloat(Swift.max(plan.rows.count - 1, 0))
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? CGFloat(natural(subviews))
        return CGSize(width: width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let plan = plan(width: bounds.width, subviews: subviews)
        var y = bounds.minY
        for (row, height) in plan.rows.enumerated() {
            for column in 0..<plan.count {
                let index = row * plan.count + column
                guard index < subviews.count else { break }
                let x = bounds.minX + CGFloat(column) * (plan.column + CGFloat(gap))
                let fills = subviews[index][FillsRow.self]
                subviews[index].place(
                    at: CGPoint(x: x, y: y),
                    anchor: .topLeading,
                    proposal: ProposedViewSize(width: plan.column, height: fills ? height : nil)
                )
            }
            y += height + CGFloat(rowGap)
        }
    }
}

// MARK: Carousels

/// Parts that scroll across, stopping at a part. With a least width, as many
/// as fit show at once, each the same width, and when more are waiting the
/// next one shows the edge of its card, never its words; on a screen's ground
/// the run goes to the window's edges
/// while its first part starts at the content's leading edge, as the App
/// Store's shelves do. Cards in a run share one height. Paged, one part at a
/// time with the system's page marks. VoiceOver steps through every part.
struct CarouselPart: View {
    let carousel: Navree.Carousel
    @Environment(\.navreeScreenMargin) private var margin
    @State private var run: Double?

    /// The room inside the margins, from the run's own width.
    private var room: Double? {
        run.map { Swift.max($0 - 2 * Double(margin ?? 0), 0) }
    }

    var body: some View {
        if carousel.paged {
            PagedCarousel(carousel: carousel)
        } else {
            let gap = CompositionRules.points(steps: carousel.gap)
            let bleed = margin ?? 0
            let layout = CompositionRules.carousel(
                room: room ?? 0, min: carousel.min, gap: gap, parts: carousel.children.count, margin: Double(bleed),
                edge: CompositionRules.surfaceEdge(carousel.children)
            )
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: CGFloat(gap)) {
                    ForEach(Array(PartItem.of(carousel.children).enumerated()), id: \.element.id) { _, item in
                        NodeView(item.node)
                            .frame(width: layout.width.map { CGFloat($0) }, alignment: .topLeading)
                            .frame(maxHeight: .infinity, alignment: .top)
                            .environment(\.navreeInTileRow, CompositionRules.fillsRow(item.node))
                            .modifier(Arriving(node: item.node))
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                .scrollTargetLayout()
            }
            .contentMargins(.horizontal, bleed, for: .scrollContent)
            .scrollTargetBehavior(.viewAligned)
            .scrollIndicators(.hidden)
            .scrollDisabled(layout.width != nil && !CompositionRules.scrolls(shown: layout.shown, parts: carousel.children.count))
            // The run as it stands, from one edge of the window to the other:
            // the room is what it has inside the margins.
            .onGeometryChange(for: Double.self) { Double($0.size.width) } action: { run = $0 }
            .padding(.horizontal, -bleed)
            .opacity(room == nil && carousel.min != nil ? 0 : 1)
            .accessibilityElement(children: .contain)
            .accessibilityLabel(carousel.label ?? "")
            .animatesParts(carousel.children)
        }
    }
}

/// A carousel that shows one part at a time, each the width of the run, with
/// the system's page marks under it (a page control). On the Mac, which has
/// none, the parts scroll as any run does.
struct PagedCarousel: View {
    let carousel: Navree.Carousel
    @State private var page: String?

    var body: some View {
        let items = PartItem.of(carousel.children)
        VStack(spacing: 8) {
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: 0) {
                    ForEach(items) { item in
                        NodeView(item.node)
                            .containerRelativeFrame(.horizontal)
                            .frame(maxHeight: .infinity, alignment: .top)
                            .environment(\.navreeInTileRow, CompositionRules.fillsRow(item.node))
                            .id(item.id)
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                .scrollTargetLayout()
            }
            .scrollTargetBehavior(.paging)
            .scrollIndicators(.hidden)
            .scrollPosition(id: $page)
            #if os(iOS)
            if items.count > 1 {
                PageMarks(
                    count: items.count,
                    current: Binding(
                        get: { page.flatMap { id in items.firstIndex { $0.id == id } } ?? 0 },
                        set: { index in withAnimation { page = items[index].id } }
                    )
                )
                .fixedSize()
                .accessibilityHidden(true)
            }
            #endif
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(carousel.label ?? "")
    }
}

#if os(iOS)
/// The system's page control: the marks that say which page shows, and that
/// turn the page when tapped. Its marks take the label colours, so they read
/// on the grouped ground in light and dark.
struct PageMarks: UIViewRepresentable {
    let count: Int
    @Binding var current: Int

    func makeUIView(context: Context) -> UIPageControl {
        let control = UIPageControl()
        control.pageIndicatorTintColor = .tertiaryLabel
        control.currentPageIndicatorTintColor = .label
        control.hidesForSinglePage = true
        control.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .valueChanged)
        return control
    }

    func updateUIView(_ control: UIPageControl, context: Context) {
        control.numberOfPages = count
        control.currentPage = current
        context.coordinator.current = $current
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(current: $current)
    }

    @MainActor
    final class Coordinator: NSObject {
        var current: Binding<Int>

        init(current: Binding<Int>) {
            self.current = current
        }

        @objc func changed(_ control: UIPageControl) {
            current.wrappedValue = control.currentPage
        }
    }
}
#endif

// MARK: The drawing's knowledge of where a part stands

extension EnvironmentValues {
    /// The axis of the stack the parts being drawn stand in.
    @Entry var navreeParentAxis: Navree.Axis = .vertical
    /// The most columns of the grid the parts being drawn stand in.
    @Entry var navreeGridColumns: Int? = nil
    /// The width of the room a composed screen's content has, in points, when
    /// the parts being drawn stand in one that measured it.
    @Entry var navreeRoomWidth: Double? = nil
    /// The margin of the composed screen, when the parts being drawn stand on
    /// its ground (not in a card): a run of cards there goes to the edges.
    @Entry var navreeScreenMargin: CGFloat? = nil
    /// Whether the parts being drawn stand on a composed screen's ground
    /// rather than on a surface.
    @Entry var navreeOnGround: Bool = false
    /// Whether the part being drawn is a cell of a row of tiles, as high as
    /// the row.
    @Entry var navreeInTileRow: Bool = false
    /// Whether the parts being drawn stand in a cell of a grid, beside
    /// siblings that share their type size: a word kept whole there is never
    /// shrunk, the grid gives it room.
    @Entry var navreeInTiles: Bool = false
}
