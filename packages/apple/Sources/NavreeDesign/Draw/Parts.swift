import NavreeProtocol
import SwiftUI

/// Draws any part as free content, with the system's own views. Rows of lists
/// are drawn by ``RowView``; this is what goes inside them and anywhere else.
public struct NodeView: View {
    let node: Navree.Node

    public init(_ node: Navree.Node) {
        self.node = node
    }

    public var body: some View {
        switch node {
        case .stack(let stack): StackPart(stack: stack)
        case .grid(let grid): GridPart(grid: grid)
        case .section(let section):
            if CompositionRules.isGroup(section) { GroupPart(section: section) } else { SectionPart(section: section) }
        case .list(let list):
            VStack(alignment: .leading, spacing: 10) {
                ForEach(Array(list.children.enumerated()), id: \.offset) { index, child in
                    if index > 0 { Divider() }
                    NodeView(child)
                }
            }
        case .row(let row): RowContent(row: row)
        case .split(let split):
            VStack(alignment: .leading, spacing: 16) {
                NodeView(split.primary)
                NodeView(split.detail)
            }
        case .tabs(let tabs): TabsPart(tabs: tabs)
        case .disclosure(let disclosure): DisclosurePart(disclosure: disclosure)
        case .divider: Divider()
        case .spacer: Spacer(minLength: 0)
        case .region(let region): RegionPart(region: region)
        case .text(let text): TextPart(text: text)
        case .rich(let rich): RichPart(rich: rich)
        case .image(let image): ImagePart(image: image)
        case .icon(let icon): IconPart(icon: icon)
        case .avatar(let avatar): AvatarPart(avatar: avatar)
        case .badge(let badge): StatusLabel(badge: badge)
        case .progress(let progress): ProgressPart(progress: progress)
        case .banner(let banner): BannerPart(banner: banner)
        case .empty(let empty): EmptyPart(empty: empty)
        case .facts(let facts):
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(facts.items.enumerated()), id: \.offset) { _, fact in FactContent(fact: fact) }
            }
        case .table(let table):
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(table.rows.enumerated()), id: \.offset) { _, record in
                    RecordContent(row: RecordRules.row(columns: table.columns, record: record))
                }
            }
        case .chart(let chart): ChartPart(chart: chart)
        case .code(let code): CodePart(code: code)
        case .button(let button): ButtonPart(button: button)
        case .link(let link): LinkPart(link: link)
        case .menu(let menu): MenuPart(menu: menu)
        case .form(let form):
            VStack(alignment: .leading, spacing: 12) {
                ForEach(Array(form.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
            }
            .environment(\.navreeForm, FormRef(form))
        case .field(let field): FieldControl(field: field)
        case .sheet, .dialog, .popover: EmptyView()
        case .native(let view):
            if let alt = view.alt { NodeView(alt) }
        case .web:
            Label("Open this part on the web", systemImage: "safari")
                .foregroundStyle(.secondary)
        case .unknown(let part):
            if let alt = part.alt { NodeView(alt) }
        case .gauge(let gauge): GaugePart(gauge: gauge)
        case .figure(let figure): FigurePart(figure: figure)
        case .timeline(let timeline): TimelinePart(timeline: timeline)
        case .ring(let ring): RingPart(ring: ring)
        case .rings(let rings): RingsPart(rings: rings)
        case .dayClock(let clock): DayClockPart(clock: clock)
        case .dayPager(let pager): DayPagerView(pager: pager)
        case .meter(let meter): MeterPart(meter: meter)
        case .columns(let columns): ColumnsPart(columns: columns)
        case .strip(let strip): StripPart(strip: strip)
        case .range(let range): RangePart(range: range)
        case .calendarHeatmap(let heatmap): CalendarHeatmapPart(heatmap: heatmap)
        case .matrixHeatmap(let matrix): MatrixHeatmapPart(matrix: matrix)
        case .scatter(let scatter): ScatterPart(scatter: scatter)
        case .card(let card): CardPart(card: card)
        case .layers(let layers): LayersPart(layers: layers)
        case .shape(let shape): ShapePart(shape: shape)
        case .carousel(let carousel): CarouselPart(carousel: carousel)
        case .adaptive(let adaptive): AdaptivePart(adaptive: adaptive)
        case .toolbar, .feedRow, .message, .pane:
            // Kinds this client does not draw yet.
            EmptyView()
        }
    }
}

/// Parts one after another. Across: they wrap onto lines of their own when
/// the text is large or the room runs out, rather than squeezing.
struct StackPart: View {
    let stack: Navree.Stack
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.navreeParentAxis) private var parentAxis

    var body: some View {
        if stack.hidden {
            EmptyView()
        } else {
            let turned = stack.wide != nil && WidthClass(sizeClass) == .regular
            laid(turned: turned)
                .padding(CGFloat(CompositionRules.points(steps: stack.pad)))
                .frame(maxWidth: fills ? .infinity : nil, alignment: frameAlignment)
        }
    }

    /// A stack that grows takes the room left; a vertical stack that centres
    /// its parts is as wide as its place, so they stand in its middle.
    private var fills: Bool {
        stack.grow || (parentAxis == .vertical && stack.align == .center && CompositionRules.axis(stack, width: WidthClass(sizeClass)) == .vertical)
    }

    private var frameAlignment: Alignment {
        stack.align == .center ? .center : stack.align == .end ? .trailing : .leading
    }

    @ViewBuilder private func laid(turned: Bool) -> some View {
        let axis = CompositionRules.axis(stack, width: WidthClass(sizeClass))
        if axis == .vertical {
            VStack(alignment: horizontal, spacing: spacing) {
                children
            }
            .environment(\.navreeParentAxis, .vertical)
        } else if turned || (stack.children.contains(where: Self.grows) && !typeSize.isAccessibilitySize) {
            // Turned across by a wide room, or holding a part that takes the
            // room left (its words wrap there): no fallback.
            HStack(alignment: vertical, spacing: spacing) {
                children
            }
            .environment(\.navreeParentAxis, .horizontal)
        } else if typeSize.isAccessibilitySize {
            VStack(alignment: .leading, spacing: spacing) {
                children
            }
        } else {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: vertical, spacing: spacing) {
                    if stack.justify == .between, stack.children.count == 2 {
                        NodeView(stack.children[0])
                        Spacer(minLength: 8)
                        NodeView(stack.children[1])
                    } else {
                        children
                    }
                }
                VStack(alignment: .leading, spacing: spacing) {
                    children
                }
            }
            .environment(\.navreeParentAxis, .horizontal)
        }
    }

    @ViewBuilder private var children: some View {
        ForEach(Array(stack.children.enumerated()), id: \.offset) { index, child in
            NodeView(child).environment(\.navreeWordsAbove, HeadingRules.wordsAbove(stack.children, index: index))
        }
    }

    static func grows(_ node: Navree.Node) -> Bool {
        if case .stack(let stack) = node { return stack.grow }
        return false
    }

    private var spacing: CGFloat? {
        stack.gap == 0 ? nil : CGFloat(stack.gap) * 4
    }

    private var horizontal: HorizontalAlignment {
        switch stack.align {
        case .center: .center
        case .end: .trailing
        default: .leading
        }
    }

    private var vertical: VerticalAlignment {
        switch stack.align {
        case .start: .top
        case .end: .bottom
        case .baseline: .firstTextBaseline
        default: .center
        }
    }
}

/// Parts in as many columns as fit.
struct GridPart: View {
    let grid: Navree.Grid

    var body: some View {
        let gap = grid.gap > 0 ? CGFloat(CompositionRules.points(steps: grid.gap)) : 12
        LazyVGrid(columns: columns(gap: gap), alignment: .leading, spacing: max(gap, 12)) {
            ForEach(Array(grid.children.enumerated()), id: \.offset) { _, child in
                NodeView(child).frame(maxWidth: .infinity, alignment: .topLeading)
            }
        }
        .frame(maxWidth: .infinity)
    }

    private func columns(gap: CGFloat) -> [GridItem] {
        switch CompositionRules.columns(grid) {
        case .fixed(let count):
            Array(repeating: GridItem(.flexible(), spacing: gap, alignment: .top), count: count)
        case .fitting(let minimum):
            [GridItem(.adaptive(minimum: CGFloat(minimum)), spacing: gap, alignment: .top)]
        }
    }
}

/// A section drawn as free content: a heading in the headline style and its
/// parts under it. In a screen's list, sections become the list's own.
struct SectionPart: View {
    let section: Navree.Section

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title = section.title {
                Text(title).font(.headline).accessibilityAddTraits(.isHeader)
            }
            if let subtitle = section.subtitle {
                Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
            }
            ForEach(Array((section.actions + section.children + section.footer).enumerated()), id: \.offset) { _, child in
                NodeView(child)
            }
        }
    }
}

/// Text, in the system's text style for what it is for, so it follows the
/// reader's text size.
struct TextPart: View {
    let text: Navree.Text

    @ScaledMetric(relativeTo: .largeTitle) private var display: CGFloat = 48
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Text(text.value)
            .font(font)
            .fontWeight(text.weight.map(Typography.weight))
            .foregroundStyle(colour)
            .lineLimit(text.lines)
            .multilineTextAlignment(Typography.alignment(text.align))
            .frame(maxWidth: text.align == .center || text.align == .end ? .infinity : nil, alignment: Typography.frame(text.align))
            .fixedSize(horizontal: false, vertical: text.lines == nil)
            .accessibilityAddTraits(text.heading != nil ? .isHeader : [])
            .accessibilityHeading(Typography.heading(text.heading))
    }

    private var font: Font {
        let font = text.style == .display ? Font.system(size: display, weight: .bold, design: .rounded) : Typography.font(text.style)
        return text.tabular ? font.monospacedDigit() : font
    }

    private var colour: Color {
        if let colour = text.colour { return Hues.text(colour, scheme: scheme) }
        return text.tone.map(Tones.color) ?? (text.muted ? Color.secondary : Color.primary)
    }
}

/// Text in runs, with links the reader can follow.
struct RichPart: View {
    let rich: Navree.Rich
    @Environment(\.navree) private var context

    var body: some View {
        Text(attributed)
            .font(Typography.font(rich.style))
            .foregroundStyle(rich.muted ? Color.secondary : Color.primary)
            .environment(\.openURL, OpenURLAction { url in
                context.send(.act(.navigate(Navree.Action.Navigate(href: url.absoluteString, present: .push)), Place()))
                return .handled
            })
    }

    private var attributed: AttributedString {
        var result = AttributedString()
        for run in rich.runs {
            var piece = AttributedString(run.text)
            var intent: InlinePresentationIntent = []
            if run.bold { intent.insert(.stronglyEmphasized) }
            if run.italic { intent.insert(.emphasized) }
            if run.code { intent.insert(.code) }
            if !intent.isEmpty { piece.inlinePresentationIntent = intent }
            if let href = run.href, let url = URL(string: href) { piece.link = url }
            result += piece
        }
        return result
    }
}

/// A picture, with its words for VoiceOver, or hidden from it when it is
/// decoration.
struct ImagePart: View {
    let image: Navree.Image
    @Environment(\.navree) private var context

    var body: some View {
        AsyncImage(url: URL(string: image.src, relativeTo: context.base)) { loading in
            if let loaded = loading.image {
                loaded.resizable().aspectRatio(contentMode: image.fit == .cover ? .fill : .fit)
            } else {
                Rectangle().fill(.quaternary)
            }
        }
        .frame(width: image.width.map { CGFloat($0) }, height: image.height.map { CGFloat($0) })
        .clipped()
        .accessibilityLabel(image.alt)
        .accessibilityHidden(image.alt.isEmpty)
    }
}

/// An icon: the system symbol that means the same, or the app's own shape
/// drawn like one.
struct IconPart: View {
    let icon: Navree.Icon
    @Environment(\.navree) private var context

    var body: some View {
        Glyph(icon: icon, icons: context.icons)
            .font(icon.scale.map(Typography.font))
            .foregroundStyle(icon.colour.map { Hues.color($0) } ?? icon.tone.map(Tones.color) ?? Color.secondary)
            .accessibilityLabel(icon.label ?? "")
            .accessibilityHidden(icon.label == nil)
    }
}

/// A symbol for an icon, or its shape when no symbol means the same.
public struct Glyph: View {
    let icon: Navree.Icon
    let icons: [String: Navree.IconShape]

    public init(icon: Navree.Icon, icons: [String: Navree.IconShape] = [:]) {
        self.icon = icon
        self.icons = icons
    }

    public var body: some View {
        if let symbol = Symbols.symbol(for: icon) {
            Image(systemName: symbol)
        } else {
            let shape = icon.name.flatMap { icons[$0] }
            IconShapeView(
                paths: icon.paths.isEmpty ? shape?.paths ?? [] : icon.paths,
                fills: icon.fills.isEmpty ? shape?.fills ?? [] : icon.fills,
                viewBox: icon.viewBox ?? shape?.viewBox
            )
        }
    }
}

/// A person's picture, or their initials in a circle.
struct AvatarPart: View {
    let avatar: Navree.Avatar
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 32

    var body: some View {
        Group {
            if let src = avatar.src, let url = URL(string: src) {
                AsyncImage(url: url) { image in image.resizable().scaledToFill() } placeholder: { initials }
            } else {
                initials
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .accessibilityLabel(avatar.name)
    }

    private var initials: some View {
        ZStack {
            Circle().fill(.quaternary)
            if let letters = avatar.initials {
                Text(letters).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            } else {
                Image(systemName: "person.crop.circle.fill").resizable().foregroundStyle(.secondary)
            }
        }
    }
}

/// A state, as Apple's apps show one: words in the tone's colour beside a
/// symbol (a dot for a plain state), never a coloured pill.
public struct StatusLabel: View {
    let badge: Navree.Badge

    public init(badge: Navree.Badge) {
        self.badge = badge
    }

    @ScaledMetric(relativeTo: .caption) private var dot: CGFloat = 7

    public var body: some View {
        HStack(spacing: 5) {
            if badge.dot {
                Circle().fill(Tones.color(badge.tone)).frame(width: dot, height: dot)
            } else {
                Image(systemName: Tones.symbol(badge.tone)).foregroundStyle(Tones.color(badge.tone))
            }
            Text(badge.text)
        }
        .accessibilityElement(children: .combine)
    }
}

/// How far something has got: a bar when it is known, a spinner when not.
struct ProgressPart: View {
    let progress: Navree.Progress
    @Environment(\.navreeInRowDetail) private var inRow

    var body: some View {
        if let value = progress.value {
            ProgressView(value: Double(value), total: 1000) {
                if let label = progress.label, !inRow { Text(label) }
            }
            .tint(tint)
            .accessibilityLabel(progress.label ?? "")
        } else {
            ProgressView { if let label = progress.label { Text(label) } }
        }
    }

    /// The measure's colour, else the colour of what it means, else the app's tint.
    private var tint: Color {
        switch ProgressRules.fill(progress) {
        case .measure(let colour): Hues.color(colour)
        case .tone(let tone): Hues.color(tone)
        case .accent: .accentColor
        }
    }
}

/// Code in the fixed-width face, scrolled across and selectable.
struct CodePart: View {
    let code: Navree.Code

    var body: some View {
        ScrollView(.horizontal) {
            Text(code.text).font(.callout.monospaced()).textSelection(.enabled).padding(.vertical, 4)
        }
    }
}

/// Views a reader switches between inside a screen: a segmented control (a
/// tab view is for the app's top level only).
struct TabsPart: View {
    let tabs: Navree.Tabs
    @Environment(\.navree) private var context
    @State private var chosen: Int?

    var body: some View {
        let selected = chosen ?? tabs.items.firstIndex(where: \.selected) ?? 0
        VStack(alignment: .leading, spacing: 12) {
            Picker("", selection: Binding(get: { selected }, set: { index in
                chosen = index
                if let href = tabs.items[index].href {
                    context.send(.act(.navigate(Navree.Action.Navigate(href: href, present: .replace)), Place()))
                }
            })) {
                ForEach(Array(tabs.items.enumerated()), id: \.offset) { index, tab in Text(tab.label).tag(index) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if selected < tabs.items.count {
                ForEach(Array(tabs.items[selected].children.enumerated()), id: \.offset) { _, child in NodeView(child) }
            }
        }
    }
}

/// A part that opens under its label.
struct DisclosurePart: View {
    let disclosure: Navree.Disclosure
    @State private var open: Bool?

    var body: some View {
        DisclosureGroup(disclosure.label, isExpanded: Binding(get: { open ?? disclosure.open }, set: { open = $0 })) {
            ForEach(Array(disclosure.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
    }
}

/// A region drawn as free content: a spinner until it is fetched.
struct RegionPart: View {
    let region: Navree.Region
    @Environment(\.navree) private var context

    var body: some View {
        if region.children.isEmpty {
            ProgressView()
                .frame(maxWidth: .infinity)
                .task { context.send(.appear(region: region.id)) }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(region.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
            }
        }
    }
}

/// A message in place: a symbol in its tone, its words, and what can be done.
struct BannerPart: View {
    let banner: Navree.Banner

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 4) {
                if let title = banner.title { Text(title).font(.headline) }
                if let text = banner.text { Text(text).font(.subheadline).foregroundStyle(.secondary) }
                ForEach(Array(banner.actions.enumerated()), id: \.offset) { _, action in NodeView(action) }
            }
        } icon: {
            Image(systemName: Tones.symbol(banner.tone)).foregroundStyle(Tones.color(banner.tone))
        }
        .accessibilityElement(children: .combine)
    }
}

/// What a place shows when it holds nothing: the system's own layout.
struct EmptyPart: View {
    let empty: Navree.Empty

    var body: some View {
        ContentUnavailableView {
            Label {
                Text(empty.title)
            } icon: {
                if let icon = empty.icon { Glyph(icon: icon) } else { Image(systemName: "tray") }
            }
        } description: {
            if let text = empty.text { Text(text) }
        } actions: {
            ForEach(Array(empty.actions.enumerated()), id: \.offset) { _, action in
                if case .button(let button) = action {
                    ButtonPart(button: button, prominent: true)
                } else {
                    NodeView(action)
                }
            }
        }
    }
}

/// The type the system has for each text style. Fixed sizes are never used,
/// so every text follows the reader's size.
public enum Typography {
    public static func font(_ style: Navree.TextStyle) -> Font {
        switch style {
        case .display, .largeTitle: .largeTitle
        case .title: .title
        case .title2: .title2
        case .headline: .headline
        case .body: .body
        case .callout: .callout
        case .caption: .caption
        case .mono: .body.monospaced()
        }
    }

    static func weight(_ weight: Navree.Weight) -> Font.Weight {
        switch weight {
        case .regular: .regular
        case .medium: .medium
        case .semibold: .semibold
        case .bold: .bold
        }
    }

    static func alignment(_ align: Navree.Align?) -> TextAlignment {
        switch align {
        case .center: .center
        case .end: .trailing
        default: .leading
        }
    }

    static func frame(_ align: Navree.Align?) -> Alignment {
        switch align {
        case .center: .center
        case .end: .trailing
        default: .leading
        }
    }

    static func heading(_ level: Int?) -> AccessibilityHeadingLevel {
        switch level {
        case 1: .h1
        case 2: .h2
        case 3: .h3
        case 4: .h4
        case 5: .h5
        case 6: .h6
        default: .unspecified
        }
    }
}
