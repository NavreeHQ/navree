import NavreeProtocol
import SwiftUI

/// Draws any part as free content, with the system's own views. Rows of lists
/// are drawn by ``RowView``; this is what goes inside them and anywhere else.
public struct NodeView: View {
    let node: Node

    public init(_ node: Node) {
        self.node = node
    }

    public var body: some View {
        switch node {
        case .stack(let stack): StackPart(stack: stack)
        case .grid(let grid): GridPart(grid: grid)
        case .section(let section): SectionPart(section: section)
        case .list(let list):
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(list.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
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
        }
    }
}

/// Parts one after another. Across: they wrap onto lines of their own when
/// the text is large or the room runs out, rather than squeezing.
struct StackPart: View {
    let stack: Node.Stack
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        if stack.hidden {
            EmptyView()
        } else if stack.axis == .vertical {
            VStack(alignment: horizontal, spacing: spacing) {
                children
            }
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
        }
    }

    @ViewBuilder private var children: some View {
        ForEach(Array(stack.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
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
    let grid: Node.Grid

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: CGFloat(grid.min ?? 160)), spacing: 12)], alignment: .leading, spacing: 12) {
            ForEach(Array(grid.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
    }
}

/// A section drawn as free content: a heading in the headline style and its
/// parts under it. In a screen's list, sections become the list's own.
struct SectionPart: View {
    let section: Node.Section

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
    let text: Node.Text

    var body: some View {
        Text(text.value)
            .font(Typography.font(text.style))
            .foregroundStyle(text.tone.map(Tones.color) ?? (text.muted ? Color.secondary : Color.primary))
            .lineLimit(text.lines)
            .accessibilityAddTraits(text.heading != nil ? .isHeader : [])
            .accessibilityHeading(Typography.heading(text.heading))
    }
}

/// Text in runs, with links the reader can follow.
struct RichPart: View {
    let rich: Node.Rich
    @Environment(\.navree) private var context

    var body: some View {
        Text(attributed)
            .font(Typography.font(rich.style))
            .foregroundStyle(rich.muted ? Color.secondary : Color.primary)
            .environment(\.openURL, OpenURLAction { url in
                context.send(.act(.navigate(href: url.absoluteString, present: .push), Place()))
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
    let image: Node.Image
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
    let icon: Node.Icon
    @Environment(\.navree) private var context

    var body: some View {
        Glyph(icon: icon, icons: context.icons)
            .foregroundStyle(icon.tone.map(Tones.color) ?? Color.secondary)
            .accessibilityLabel(icon.label ?? "")
            .accessibilityHidden(icon.label == nil)
    }
}

/// A symbol for an icon, or its shape when no symbol means the same.
public struct Glyph: View {
    let icon: Node.Icon
    let icons: [String: IconShape]

    public init(icon: Node.Icon, icons: [String: IconShape] = [:]) {
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
    let avatar: Node.Avatar
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
    let badge: Node.Badge

    public init(badge: Node.Badge) {
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
    let progress: Node.Progress

    var body: some View {
        if let value = progress.value {
            ProgressView(value: Double(value), total: 1000) {
                if let label = progress.label { Text(label) }
            }
        } else {
            ProgressView { if let label = progress.label { Text(label) } }
        }
    }
}

/// Code in the fixed-width face, scrolled across and selectable.
struct CodePart: View {
    let code: Node.Code

    var body: some View {
        ScrollView(.horizontal) {
            Text(code.text).font(.callout.monospaced()).textSelection(.enabled).padding(.vertical, 4)
        }
    }
}

/// Views a reader switches between inside a screen: a segmented control (a
/// tab view is for the app's top level only).
struct TabsPart: View {
    let tabs: Node.Tabs
    @Environment(\.navree) private var context
    @State private var chosen: Int?

    var body: some View {
        let selected = chosen ?? tabs.items.firstIndex(where: \.selected) ?? 0
        VStack(alignment: .leading, spacing: 12) {
            Picker("", selection: Binding(get: { selected }, set: { index in
                chosen = index
                if let href = tabs.items[index].href {
                    context.send(.act(.navigate(href: href, present: .replace), Place()))
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
    let disclosure: Node.Disclosure
    @State private var open: Bool?

    var body: some View {
        DisclosureGroup(disclosure.label, isExpanded: Binding(get: { open ?? disclosure.open }, set: { open = $0 })) {
            ForEach(Array(disclosure.children.enumerated()), id: \.offset) { _, child in NodeView(child) }
        }
    }
}

/// A region drawn as free content: a spinner until it is fetched.
struct RegionPart: View {
    let region: Node.Region
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
    let banner: Node.Banner

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
    let empty: Node.Empty

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
    public static func font(_ style: TextStyle) -> Font {
        switch style {
        case .largeTitle: .largeTitle
        case .title: .title2
        case .headline: .headline
        case .body: .body
        case .callout: .callout
        case .caption: .caption
        case .mono: .body.monospaced()
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
