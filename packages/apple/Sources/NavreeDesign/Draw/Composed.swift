import NavreeProtocol
import SwiftUI

// A first drawing of the composition kinds, as much of them as the health
// screens need: a card as a surface of content, a section of one card as a
// group with its heading on the ground, parts over each other, a shape, the
// two rooms of an adaptive part, and parts that scroll across.

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

/// The parts of a composed screen, set out at the screen's margins, without
/// the scroll view that holds them.
public struct ComposedContent: View {
    let node: Navree.Node

    public init(body: Navree.Node) {
        node = body
    }

    public var body: some View {
        NodeView(node)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity)
            .background(Surfaces.ground)
    }
}

/// A card: a surface of content on the grouped ground, its parts one under
/// another. Tapped as a whole, it is one control and nothing in it takes a
/// tap of its own.
struct CardPart: View {
    let card: Navree.Card
    @Environment(\.navreeGridColumns) private var columns
    @Environment(\.navree) private var context
    @Environment(\.navreeInControl) private var inControl
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        if let action = card.action, !inControl {
            Button {
                context.send(.act(action, Place(from: card.id)))
            } label: {
                surface.contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            }
            .buttonStyle(.plain)
            .environment(\.navreeInControl, true)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(card.label ?? "")
            .contextMenu { if !card.menu.isEmpty { MenuItems(items: card.menu) } }
        } else {
            surface
                .contextMenu { if !card.menu.isEmpty { MenuItems(items: card.menu) } }
        }
    }

    private var surface: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(card.children.enumerated()), id: \.offset) { index, child in
                NodeView(child).environment(\.navreeWordsAbove, HeadingRules.wordsAbove(card.children, index: index))
            }
        }
        .padding(CompositionRules.padding(card, columns: columns))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(ground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    private var ground: AnyShapeStyle {
        if let colour = card.colour {
            return AnyShapeStyle(Hues.color(colour, scheme: scheme).opacity(0.15))
        }
        return AnyShapeStyle(Surfaces.card)
    }
}

/// A section that holds one card: its heading on the ground, as a grouped
/// list sets a section's header (the title, the line under it, its hint and
/// its actions at the end), and the card under it.
struct GroupPart: View {
    let section: Navree.Section

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
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
                .padding(.horizontal, 4)
            }
            ForEach(Array((section.children + section.footer).enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
    }
}

/// Parts over each other, the first at the back, all at one place.
struct LayersPart: View {
    let layers: Navree.Layers
    @ScaledMetric(relativeTo: .body) private var scale: CGFloat = 1

    var body: some View {
        let factor = layers.size?.scaled == true ? scale : 1
        ZStack(alignment: alignment) {
            ForEach(Array(layers.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
        .frame(
            width: layers.size?.width.map { CGFloat($0) * factor },
            height: layers.size?.height.map { CGFloat($0) * factor }
        )
        .padding(CGFloat(CompositionRules.points(steps: layers.pad)))
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

/// A shape: a filled form in a hue, soft when it is the hue's tinted ground.
struct ShapePart: View {
    let shape: Navree.Shape
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let fill = shape.fill.map { Hues.color($0, scheme: scheme) } ?? Color.secondary
        let style: AnyShapeStyle = shape.fill?.to != nil
            ? AnyShapeStyle(LinearGradient(colors: [fill, Hues.end(shape.fill)], startPoint: .topLeading, endPoint: .bottomTrailing))
            : AnyShapeStyle(fill.opacity(shape.soft ? 0.18 : 1))
        form(style)
            .frame(width: shape.size?.width.map { CGFloat($0) }, height: shape.size?.height.map { CGFloat($0) })
            .accessibilityLabel(shape.label ?? "")
            .accessibilityHidden(shape.label == nil)
    }

    @ViewBuilder private func form(_ style: AnyShapeStyle) -> some View {
        switch shape.form {
        case .rounded: RoundedRectangle(cornerRadius: 12, style: .continuous).fill(style)
        case .circle: Circle().fill(style)
        case .capsule: Capsule().fill(style)
        case .rect: Rectangle().fill(style)
        }
    }
}

/// One of two rooms: the wide one in a regular width, the other otherwise.
struct AdaptivePart: View {
    let adaptive: Navree.Adaptive
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(CompositionRules.room(adaptive, width: WidthClass(sizeClass)).enumerated()), id: \.offset) { _, child in
                NodeView(child)
            }
        }
    }
}

/// Parts that scroll across, each as wide as its share of the room.
struct CarouselPart: View {
    let carousel: Navree.Carousel

    var body: some View {
        ScrollView(.horizontal) {
            LazyHStack(alignment: .top, spacing: CGFloat(CompositionRules.points(steps: carousel.gap))) {
                ForEach(Array(carousel.children.enumerated()), id: \.offset) { _, child in
                    NodeView(child)
                        .frame(width: carousel.min.map { CGFloat($0) })
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.viewAligned)
        .scrollIndicators(.hidden)
        .accessibilityLabel(carousel.label ?? "")
    }
}

extension EnvironmentValues {
    /// The axis of the stack the parts being drawn stand in.
    @Entry var navreeParentAxis: Navree.Axis = .vertical
    /// The fixed number of columns of the grid the parts being drawn stand in.
    @Entry var navreeGridColumns: Int? = nil
}
