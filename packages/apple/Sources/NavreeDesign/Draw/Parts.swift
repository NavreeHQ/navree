import NavreeProtocol
import SwiftUI

/// Draws any part as free content, with the system's own views. Rows of lists
/// are drawn by ``RowView``; this is what goes inside them and anywhere else.
public struct NodeView: View {
    let node: Navree.Node
    @Environment(\.navreeOnGround) private var onGround

    public init(_ node: Navree.Node) {
        self.node = node
    }

    public var body: some View {
        switch node {
        case .stack(let stack): StackPart(stack: stack)
        case .grid(let grid): GridPart(grid: grid)
        case .section(let section):
            if CompositionRules.isGroup(section) {
                GroupPart(section: section)
            } else if onGround {
                GroundSectionPart(section: section)
            } else {
                SectionPart(section: section)
            }
        case .list(let list): ListPart(list: list)
        case .row(let row): RowContent(row: row)
        case .split(let split): InPlaceSplit(split: split)
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
        case .badge(let badge): StatusLabel(badge: badge).font(.subheadline)
        case .progress(let progress): ProgressPart(progress: progress)
        case .banner(let banner): BannerPart(banner: banner)
        case .empty(let empty): EmptyPart(empty: empty)
        case .facts(let facts) where !onGround:
            // On a surface, labelled rows as a list's rows stand, with the
            // system's separators between them, as Settings and Wallet set them.
            VStack(alignment: .leading, spacing: 10) {
                ForEach(Array(facts.items.enumerated()), id: \.offset) { index, fact in
                    if index > 0 { Divider() }
                    FactContent(fact: fact)
                }
            }
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
        case .feedRow(let row): FeedRowPart(row: row)
        case .message(let message): MessagePart(message: message)
        case .pane(let pane): PanePart(pane: pane)
        case .toolbar:
            // A screen's toolbar is placed in its bars, never drawn in place.
            EmptyView()
        }
    }
}

/// Parts one after another. A stack that turns with the room (`wide`) keeps
/// one layout whose axis changes, so its parts keep their state. Across, a
/// part that grows takes the room left; parts that may not fit go one under
/// another when the line runs out or the text is at the accessibility sizes,
/// and a stack that wraps flows onto lines.
struct StackPart: View {
    let stack: Navree.Stack
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.navreeParentAxis) private var parentAxis
    private let room = Room()

    var body: some View {
        if stack.hidden {
            EmptyView()
        } else {
            let axis = CompositionRules.axis(stack, wide: room.wide)
            laid(axis: axis)
                .modifier(PersonSpeech(name: SpeechRules.person(in: stack)))
                .padding(CGFloat(CompositionRules.points(steps: stack.pad)))
                .frame(
                    maxWidth: CompositionRules.fillsWidth(stack, axis: axis, inLine: parentAxis == .horizontal) ? .infinity : nil,
                    alignment: frameAlignment
                )
                .animatesParts(stack.children)
        }
    }

    private var frameAlignment: Alignment {
        stack.align == .center ? .center : stack.align == .end ? .trailing : .leading
    }

    @ViewBuilder private func laid(axis: Navree.Axis) -> some View {
        if let wide = stack.wide, wide != stack.axis {
            let layout = axis == .vertical
                ? AnyLayout(VStackLayout(alignment: horizontal, spacing: spacing))
                : AnyLayout(HStackLayout(alignment: vertical, spacing: spacing))
            layout {
                Parts(nodes: stack.children, growth: axis, spread: axis == .horizontal && stack.justify == .between)
            }
            .environment(\.navreeParentAxis, axis)
        } else if axis == .vertical {
            VStack(alignment: horizontal, spacing: spacing) {
                Parts(nodes: stack.children, growth: .vertical)
            }
            .environment(\.navreeParentAxis, .vertical)
        } else if stack.wrap {
            Flow(spacing: spacing ?? 8, lineSpacing: spacing ?? 6) {
                Parts(nodes: stack.children)
            }
            .environment(\.navreeParentAxis, .horizontal)
        } else if typeSize.isAccessibilitySize {
            VStack(alignment: .leading, spacing: spacing) {
                Parts(nodes: stack.children, growth: .vertical)
            }
            .environment(\.navreeParentAxis, .vertical)
        } else if stack.children.contains(where: CompositionRules.grows) {
            // A part that takes the room left: its words wrap there, so the
            // line always fits.
            line
        } else {
            ViewThatFits(in: .horizontal) {
                line
                VStack(alignment: .leading, spacing: spacing) {
                    Parts(nodes: stack.children, growth: .vertical)
                }
                .environment(\.navreeParentAxis, .vertical)
            }
        }
    }

    private var line: some View {
        HStack(alignment: vertical, spacing: spacing) {
            if stack.justify == .end || stack.justify == .center { Spacer(minLength: 0) }
            Parts(nodes: stack.children, growth: .horizontal, spread: stack.justify == .between)
            if stack.justify == .center { Spacer(minLength: 0) }
        }
        .environment(\.navreeParentAxis, .horizontal)
    }

    private var spacing: CGFloat? {
        CompositionRules.spacing(stack.gap).map { CGFloat($0) }
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
/// reader's text size. One word or one figure stays whole on its line,
/// shrinking a little rather than breaking. When an answer changes its words,
/// a count rolls to the new number and a fade fades, with the system's own
/// timing; for a person who asked for less motion a count only fades.
struct TextPart: View {
    let text: Navree.Text

    @ScaledMetric(relativeTo: .largeTitle) private var display: CGFloat = 48
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.navreeInTiles) private var inTiles

    var body: some View {
        let whole = CompositionRules.keepsWhole(text)
        let change = CompositionRules.change(text.change, reduceMotion: reduceMotion)
        Text(text.value)
            .font(font)
            .fontWeight(text.weight.map(Typography.weight))
            .foregroundStyle(colour)
            .lineLimit(whole ? 1 : text.lines)
            // Alone, a figure too wide for its place shrinks a little; among
            // tiles it keeps its size and the grid gives it room.
            .minimumScaleFactor(whole && !inTiles ? 0.5 : 1)
            .fixedSize(horizontal: whole && inTiles, vertical: text.lines == nil && !whole)
            .multilineTextAlignment(Typography.alignment(text.align))
            .frame(maxWidth: text.align == .center || text.align == .end ? .infinity : nil, alignment: Typography.frame(text.align))
            .contentTransition(Self.transition(change))
            .animation(change == .none ? nil : .default, value: text.value)
            .accessibilityAddTraits(text.heading != nil ? .isHeader : [])
            .accessibilityHeading(Typography.heading(text.heading))
    }

    static func transition(_ change: CompositionRules.ChangeLook) -> ContentTransition {
        switch change {
        case .rolls: .numericText()
        case .fades: .opacity
        case .none: .identity
        }
    }

    private var font: Font {
        let font = text.style == .display ? Font.system(size: display, weight: .bold, design: .rounded) : Typography.font(text.style)
        return text.tabular ? font.monospacedDigit() : font
    }

    /// Words in a colour or a tone are as dark as reads on their ground.
    private var colour: AnyShapeStyle {
        if let colour = text.colour { return AnyShapeStyle(ReadableWords(Hues.text(colour, scheme: scheme))) }
        if let tone = text.tone, tone != .neutral { return AnyShapeStyle(ReadableWords(Tones.color(tone))) }
        return AnyShapeStyle(text.muted ? Color.secondary : Color.primary)
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
        RemotePicture(url: context.pictureAddress(image.src)) { loaded in
            loaded.resizable().aspectRatio(contentMode: image.fit == .cover ? .fill : .fit)
        } placeholder: {
            Rectangle().fill(.quaternary)
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
        Glyph(icon: icon, icons: context.icons, side: CompositionRules.iconSide(icon.scale), relativeTo: icon.scale.map(Typography.textStyle) ?? .body)
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
    let side: Double
    let style: Font.TextStyle

    /// `side` is the side of an icon drawn from its paths, before the
    /// person's text size, which it follows as `style` does.
    public init(icon: Navree.Icon, icons: [String: Navree.IconShape] = [:], side: Double = 20, relativeTo style: Font.TextStyle = .body) {
        self.icon = icon
        self.icons = icons
        self.side = side
        self.style = style
    }

    public var body: some View {
        if let symbol = Symbols.symbol(for: icon) {
            Image(systemName: symbol)
        } else {
            let shape = icon.name.flatMap { icons[$0] }
            IconShapeView(
                paths: icon.paths.isEmpty ? shape?.paths ?? [] : icon.paths,
                fills: icon.fills.isEmpty ? shape?.fills ?? [] : icon.fills,
                viewBox: icon.viewBox ?? shape?.viewBox,
                side: side,
                relativeTo: style
            )
        }
    }
}

/// A person's picture, or their initials in a circle.
struct AvatarPart: View {
    let avatar: Navree.Avatar
    @Environment(\.navree) private var context
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 32

    var body: some View {
        Group {
            if let src = avatar.src, let url = context.pictureAddress(src) {
                RemotePicture(url: url) { image in image.resizable().scaledToFill() } placeholder: { initials }
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
            } else if RowRules.hasSymbol(badge) {
                // The words say the state; VoiceOver would read a check
                // mark as "selected".
                Image(systemName: Tones.symbol(badge.tone)).foregroundStyle(Tones.color(badge.tone))
                    .accessibilityHidden(true)
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
    @Environment(\.navreeRowSpeech) private var row

    var body: some View {
        if let value = progress.value, let row {
            // In a row that has said its name and state: what is left of the
            // bar's words, and no percentage beside them.
            ProgressView(value: Double(value), total: 1000)
                .tint(tint)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(SpeechRules.barInRow(progress.label, title: row.title, state: row.state) ?? ""))
        } else if let value = progress.value {
            ProgressView(value: Double(value), total: 1000) {
                if let label = progress.label, !inRow { Text(label).font(.footnote).foregroundStyle(.secondary) }
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
                } else if let id = tabs.id {
                    context.send(.tab(id: id, index: index))
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
    @Environment(\.navreeRowSpeech) private var row

    var body: some View {
        DisclosureGroup(disclosure.label, isExpanded: Binding(get: { open ?? disclosure.open }, set: { open = $0 })) {
            ForEach(Array(disclosure.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
        // In a row it is one of the row's actions, not a heading inside it.
        .accessibilityHidden(row != nil)
    }
}

/// A region drawn as free content: a spinner until it is fetched.
struct RegionPart: View {
    let region: Navree.Region
    @Environment(\.navree) private var context

    var body: some View {
        if region.children.isEmpty, context.region(region.id).failed {
            Button("Try Again") { context.send(.retry(region: region.id)) }
                .buttonStyle(.bordered)
                .frame(maxWidth: .infinity)
        } else if region.children.isEmpty {
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

    /// The system's text style a style sizes with.
    static func textStyle(_ style: Navree.TextStyle) -> Font.TextStyle {
        switch style {
        case .display, .largeTitle: .largeTitle
        case .title: .title
        case .title2: .title2
        case .headline: .headline
        case .body, .mono: .body
        case .callout: .callout
        case .caption: .caption
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

/// A person's picture beside their name, said once by their whole name.
struct PersonSpeech: ViewModifier {
    let name: String?

    func body(content: Content) -> some View {
        if let name {
            content.accessibilityElement(children: .ignore).accessibilityLabel(Text(name))
        } else {
            content
        }
    }
}
