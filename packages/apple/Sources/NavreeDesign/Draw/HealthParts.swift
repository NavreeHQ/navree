import NavreeProtocol
import SwiftUI

// The small parts every health chart shares: a status in a tinted capsule, a
// trend with the system's arrow, a hint behind an info button, a key, and the
// heading of a chart.

/// How a ring or a figure says how its value stands: the word in its tone, in
/// a small capsule tinted with that tone.
struct StatusCapsule: View {
    let badge: Navree.Badge
    @ScaledMetric(relativeTo: .subheadline) private var dot: CGFloat = 7

    var body: some View {
        let color = Hues.color(badge.tone)
        HStack(spacing: 5) {
            if badge.dot {
                Circle().fill(color).frame(width: dot, height: dot)
            }
            Text(badge.text)
        }
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(ReadableWords(color))
        .padding(.horizontal, 10)
        .padding(.vertical, 3)
        .background(color.opacity(0.15), in: Capsule())
    }
}

/// Which way a value is going, with the system's arrow, in the colour of its
/// news rather than of its direction. Words too long for the line wrap beside
/// the arrow, never under it.
struct TrendLabel: View {
    let trend: Navree.Trend

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 3) {
            Image(systemName: TrendRules.symbol(trend.direction))
                .fontWeight(.semibold)
                .imageScale(.small)
            if let text = trend.text {
                Text(text)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .foregroundStyle(ReadableWords(TrendRules.hue(trend).color))
    }
}

/// More about a part, on request: the system's info button opening the page's
/// words (a popover in a wide room, a sheet in a narrow one). Inside a part
/// tapped as a whole it is not drawn: nothing there takes a tap of its own.
/// A part that is one element to VoiceOver offers the same words as one of
/// its actions, through `open`.
struct HintButton: View {
    let hint: Navree.Hint
    var open: Binding<Bool>? = nil
    @State private var shown = false
    @Environment(\.navreeInControl) private var inControl
    /// The smallest the symbol is drawn, at the person's text size.
    @ScaledMetric(relativeTo: .footnote) private var glyph: CGFloat = HitRules.smallestGlyph

    var body: some View {
        let isOpen = open ?? $shown
        let outset = CGFloat(HitRules.outset(glyph: Double(glyph)))
        if !inControl {
            Button {
                isOpen.wrappedValue = true
            } label: {
                Label(HintRules.actionName(hint), systemImage: hint.mark == .question ? "questionmark.circle" : "info.circle")
                    .labelStyle(.iconOnly)
                    // The target reaches the least Apple asks, round the
                    // symbol; the line it stands on keeps its spacing.
                    .padding(outset)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .padding(-outset)
            .popover(isPresented: isOpen) {
                HintWords(hint: hint)
            }
        }
    }
}

/// The words of a hint, as its popover or sheet shows them.
struct HintWords: View {
    let hint: Navree.Hint

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title = hint.title {
                Text(title).font(.headline)
            }
            Text(hint.text).fixedSize(horizontal: false, vertical: true)
        }
        .padding(20)
        .frame(idealWidth: 340, maxWidth: 420, alignment: .leading)
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
    }
}

/// A chart's heading: its title, and its hint beside it.
struct ChartHeading: View {
    let title: String?
    let hint: Navree.Hint?
    @Environment(\.navreeWordsAbove) private var wordsAbove

    var body: some View {
        let title = HeadingRules.showsTitle(title, wordsAbove: wordsAbove) ? title : nil
        if title != nil || hint != nil {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                if let title {
                    Text(title).font(.subheadline.weight(.semibold)).accessibilityAddTraits(.isHeader)
                }
                if let hint { HintButton(hint: hint) }
            }
        }
    }
}

/// One entry of a key: a dot in its colour, its name, and the page's words of
/// its share or total.
struct KeyEntry: View {
    let color: Color
    let name: String
    var detail: String?
    var emphasised = true
    @ScaledMetric(relativeTo: .footnote) private var dot: CGFloat = 8

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Circle().fill(color).frame(width: dot, height: dot)
            Text("\(Text(name).foregroundStyle(emphasised ? Color.primary : Color.secondary))\(Text(detail.map { "  \($0)" } ?? "").foregroundStyle(.secondary))")
        }
        .font(.footnote)
    }
}

/// A key's entries one after another, onto new lines as the room runs out.
struct KeyFlow: View {
    let entries: [(color: Color, name: String, detail: String?)]
    var current: Int? = nil

    var body: some View {
        Flow(spacing: 14, lineSpacing: 6) {
            ForEach(Array(entries.enumerated()), id: \.offset) { index, entry in
                KeyEntry(color: entry.color, name: entry.name, detail: entry.detail, emphasised: current == nil || current == index)
            }
        }
    }
}

/// Parts side by side, wrapping onto new lines when the room runs out.
struct Flow: Layout {
    var spacing: CGFloat = 8
    var lineSpacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let lines = arrange(subviews, width: width)
        let height = lines.reduce(0) { $0 + $1.height } + CGFloat(max(lines.count - 1, 0)) * lineSpacing
        let used = lines.map(\.width).max() ?? 0
        return CGSize(width: proposal.width ?? used, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for line in arrange(subviews, width: bounds.width) {
            var x = bounds.minX
            for index in line.items {
                let size = measure(subviews[index], width: bounds.width)
                subviews[index].place(at: CGPoint(x: x, y: y + (line.height - size.height) / 2), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += line.height + lineSpacing
        }
    }

    /// A part at its own width, or wrapped to the line's width when it is wider.
    private func measure(_ subview: LayoutSubview, width: CGFloat) -> CGSize {
        let ideal = subview.sizeThatFits(.unspecified)
        guard width.isFinite, ideal.width > width else { return ideal }
        return subview.sizeThatFits(ProposedViewSize(width: width, height: nil))
    }

    private struct Line {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func arrange(_ subviews: Subviews, width: CGFloat) -> [Line] {
        var lines: [Line] = [Line()]
        for index in subviews.indices {
            let size = measure(subviews[index], width: width)
            let extra = lines[lines.count - 1].items.isEmpty ? size.width : size.width + spacing
            if lines[lines.count - 1].width + extra > width, !lines[lines.count - 1].items.isEmpty {
                lines.append(Line())
            }
            let gap = lines[lines.count - 1].items.isEmpty ? 0 : spacing
            lines[lines.count - 1].items.append(index)
            lines[lines.count - 1].width += size.width + gap
            lines[lines.count - 1].height = max(lines[lines.count - 1].height, size.height)
        }
        return lines.filter { !$0.items.isEmpty }
    }
}

/// The grounds of the system's grouped screens: the page, and a card on it.
enum Surfaces {
    static var ground: Color {
        #if os(iOS)
        Color(uiColor: .systemGroupedBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    static var card: Color {
        #if os(iOS)
        Color(uiColor: .secondarySystemGroupedBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }
}

/// How large the person's text is against the default, for the sizes of
/// charts that grow with it.
struct TextScale: DynamicProperty {
    @ScaledMetric(relativeTo: .body) private var unit: CGFloat = 1

    /// The scale, no more than `cap`: a chart grows with the text until it
    /// would break.
    func capped(_ cap: Double) -> CGFloat {
        min(unit, CGFloat(cap))
    }

    /// The scale of the person's text, whatever it is.
    var full: CGFloat { unit }
}

extension EnvironmentValues {
    /// Whether the parts being drawn stand inside one control (a card or a
    /// link that is tapped as a whole), so none of them takes a tap of its own.
    @Entry var navreeInControl: Bool = false
    /// Whether the parts being drawn are the detail of a row, where a bar's
    /// words are said by the row and only read by VoiceOver.
    @Entry var navreeInRowDetail: Bool = false
    /// The words of the texts just above the part being drawn.
    @Entry var navreeWordsAbove: [String] = []
}
