import NavreeProtocol
import SwiftUI

/// A figure: one number that matters, as the Health app's summary shows one.
/// Its label with its symbol in the measure's colour, the number large in the
/// rounded face with its unit beside it, how it stands and which way it is
/// going under it, the page's line after that, a small ring or gauge at its
/// end and a small chart under it. Tapped, it opens what it is about, unless
/// it stands inside a part that is tapped as a whole.
struct FigurePart: View {
    let figure: Navree.Figure
    @Environment(\.navree) private var context
    @Environment(\.navreeInControl) private var inControl
    @Environment(\.navreeOverlay) private var overlay

    var body: some View {
        if let action = figure.action, !inControl {
            Button {
                context.send(.act(action, Place(overlay: overlay, from: figure.id)))
            } label: {
                content.contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .environment(\.navreeInControl, true)
        } else {
            content
        }
    }

    @Environment(\.colorScheme) private var scheme
    @State private var hintOpen = false

    private var hue: Color {
        Hues.text(figure.colour, fallback: figure.tone.map(Hues.color) ?? .secondary, scheme: scheme)
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline, spacing: 5) {
                        if let icon = figure.icon {
                            Glyph(icon: icon, icons: context.icons).imageScale(.small)
                        }
                        Text(figure.label).lineLimit(2)
                        if let hint = figure.hint { HintButton(hint: hint, open: $hintOpen).foregroundStyle(.secondary) }
                    }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(figure.icon != nil || figure.colour != nil ? hue : Color.secondary)
                    HStack(alignment: .firstTextBaseline, spacing: 3) {
                        Text(figure.value)
                            .font(.system(.title, design: .rounded).weight(.semibold))
                            .monospacedDigit()
                            .lineLimit(1)
                            .minimumScaleFactor(0.6)
                        if let unit = figure.unit {
                            Text(unit)
                                .font(.system(.subheadline, design: .rounded).weight(.semibold))
                                .foregroundStyle(.secondary)
                        }
                    }
                    if figure.status != nil || figure.trend != nil {
                        Flow(spacing: 8, lineSpacing: 4) {
                            if let status = figure.status { StatusLabel(badge: status).foregroundStyle(Hues.color(status.tone)) }
                            if let trend = figure.trend { TrendLabel(trend: trend) }
                        }
                        .font(.footnote.weight(.medium))
                    }
                    if let detail = figure.detail {
                        Text(detail).font(.footnote).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if let aside = figure.aside {
                    Aside(node: aside)
                }
            }
            if let chart = figure.chart {
                if case .chart(let inner) = chart {
                    ChartPart(chart: inner, small: true)
                } else {
                    NodeView(chart)
                }
            }
        }
        .accessibilityElement(children: .combine)
        .modifier(HintAction(hint: inControl ? nil : figure.hint, open: $hintOpen))
    }

    /// What stands at a figure's end: a small ring or the system's small gauge.
    private struct Aside: View {
        let node: Navree.Node

        var body: some View {
            switch node {
            case .ring(let ring):
                RingPart(ring: Self.small(ring))
            case .gauge(let gauge):
                GaugePart(gauge: gauge, aside: true)
            default:
                NodeView(node)
            }
        }

        /// A ring beside a figure's words is a small one, whatever it says.
        static func small(_ ring: Navree.Ring) -> Navree.Ring {
            var small = ring
            small.prominence = .low
            return small
        }
    }
}

/// A part that is one element to VoiceOver keeps its hint within reach: the
/// hint's info button is read with the rest, so it is offered as one of the
/// element's actions, which opens the same words.
struct HintAction: ViewModifier {
    let hint: Navree.Hint?
    let open: Binding<Bool>

    func body(content: Content) -> some View {
        if let hint {
            content.accessibilityAction(named: Text(HintRules.actionName(hint))) { open.wrappedValue = true }
        } else {
            content
        }
    }
}

/// Moments in order, as the system lists them: each with its symbol in the
/// colour of its tone, its words, and its time at the end.
struct TimelinePart: View {
    let timeline: Navree.Timeline
    @Environment(\.navree) private var context
    @ScaledMetric(relativeTo: .body) private var mark: CGFloat = 30

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(timeline.items.enumerated()), id: \.offset) { _, moment in
                HStack(alignment: .center, spacing: 12) {
                    let colour = moment.tone.map(Hues.color) ?? Color.accentColor
                    ZStack {
                        Circle().fill(colour.opacity(0.15))
                        if let icon = moment.icon {
                            Glyph(icon: icon, icons: context.icons).foregroundStyle(colour).imageScale(.small)
                        } else if let state = moment.state {
                            Image(systemName: RowContent.symbol(state)).foregroundStyle(colour)
                        }
                    }
                    .frame(width: mark, height: mark)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(moment.title).foregroundStyle(moment.muted ? Color.secondary : Color.primary)
                        if let subtitle = moment.subtitle {
                            Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
                        }
                        ForEach(Array(moment.detail.enumerated()), id: \.offset) { _, part in
                            NodeView(part).font(.footnote).foregroundStyle(.secondary)
                        }
                    }
                    Spacer(minLength: 8)
                    if let time = moment.time {
                        Text(time).monospacedDigit().foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .combine)
            }
        }
    }
}
