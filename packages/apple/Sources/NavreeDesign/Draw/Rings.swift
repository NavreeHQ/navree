import NavreeProtocol
import SwiftUI

/// One arc of a ring, in the manner of the system's activity rings: its track
/// all round in the hue made faint, the value from twelve o'clock clockwise
/// with round ends, and past the goal a second lap over the first whose end
/// casts a small shadow on the lap under it.
struct RingArc: View {
    let sweep: RingRules.Sweep?
    let start: Color
    let end: Color
    let stroke: CGFloat
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack {
            Circle()
                .inset(by: stroke / 2)
                .stroke(start.track(scheme), lineWidth: stroke)
            if let sweep {
                if sweep.past {
                    // The goal reached: a whole lap, then the part past it.
                    Circle()
                        .inset(by: stroke / 2)
                        .stroke(gradient(through: 1), lineWidth: stroke)
                    ArcShape(part: sweep.part)
                        .inset(by: stroke / 2)
                        .stroke(end, style: StrokeStyle(lineWidth: stroke, lineCap: .round))
                    Tip(part: sweep.part, stroke: stroke)
                        .fill(end)
                        .shadow(color: .black.opacity(scheme == .dark ? 0.6 : 0.35), radius: stroke * 0.18)
                } else if sweep.part > 0 {
                    ArcShape(part: sweep.part)
                        .inset(by: stroke / 2)
                        .stroke(gradient(through: sweep.part), style: StrokeStyle(lineWidth: stroke, lineCap: .round))
                    // The round start would take the colour the blend ends in.
                    Tip(part: 0, stroke: stroke).fill(start)
                }
            }
        }
    }

    private func gradient(through part: Double) -> AngularGradient {
        AngularGradient(
            colors: [start, end],
            center: .center,
            startAngle: .degrees(-90),
            endAngle: .degrees(-90 + 360 * max(part, 0.0001))
        )
    }
}

/// An arc from twelve o'clock clockwise, `part` of the way round.
struct ArcShape: InsettableShape {
    var part: Double
    var inset: CGFloat = 0

    func path(in rect: CGRect) -> Path {
        let radius = min(rect.width, rect.height) / 2 - inset
        var path = Path()
        path.addArc(
            center: CGPoint(x: rect.midX, y: rect.midY),
            radius: max(radius, 0),
            startAngle: .degrees(-90),
            endAngle: .degrees(-90 + 360 * part),
            clockwise: false
        )
        return path
    }

    func inset(by amount: CGFloat) -> ArcShape {
        var copy = self
        copy.inset += amount
        return copy
    }
}

/// The round end of an arc that is `part` of the way round.
struct Tip: Shape {
    var part: Double
    var stroke: CGFloat

    func path(in rect: CGRect) -> Path {
        let radius = min(rect.width, rect.height) / 2 - stroke / 2
        let angle = (-90 + 360 * part) * .pi / 180
        let centre = CGPoint(x: rect.midX + radius * cos(angle), y: rect.midY + radius * sin(angle))
        return Path(ellipseIn: CGRect(x: centre.x - stroke / 2, y: centre.y - stroke / 2, width: stroke, height: stroke))
    }
}

/// A ring: one value against its goal. The value and its unit stand in the
/// middle in the rounded face the system's rings use; under the ring, its
/// label (with the info button for its hint), then how it stands in a tinted
/// capsule and which way it is going. A small ring is the arc alone.
struct RingPart: View {
    let ring: Navree.Ring
    private let scale = TextScale()

    var body: some View {
        let ideal = RingRules.diameter(ring.prominence) * scale.capped(RingRules.growth(ring.prominence))
        VStack(spacing: ring.prominence == .high ? 10 : 6) {
            // As wide as the ring wants, or as its place allows when that is
            // narrower (a ring in a cell of a row of five).
            SquareUpTo(side: ideal) {
              GeometryReader { proxy in
                let diameter = min(proxy.size.width, proxy.size.height)
                let stroke = RingRules.stroke(ring.prominence, diameter: diameter)
                let colour = Hues.color(ring.colour)
                ZStack {
                    RingArc(
                        sweep: RingRules.sweep(value: ring.value, max: ring.max),
                        start: colour,
                        end: Hues.end(ring.colour),
                        stroke: stroke
                    )
                    if RingRules.showsCentre(ring.prominence, centre: ring.centre) {
                        centre(diameter: diameter, stroke: stroke, colour: colour)
                    }
                }
                .frame(width: diameter, height: diameter)
                .position(x: proxy.size.width / 2, y: proxy.size.height / 2)
              }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(ring.label ?? "")
            .accessibilityValue(ring.summary ?? RingRules.centre(ring))
            if RingRules.saysWords(ring.prominence) {
                words
            }
        }
    }

    @ViewBuilder private func centre(diameter: CGFloat, stroke: CGFloat, colour: Color) -> some View {
        let room = diameter - 2 * stroke - 12
        if ring.centre == .icon, let icon = ring.icon {
            Glyph(icon: icon)
                .font(.system(size: diameter * 0.3, weight: .semibold))
                .foregroundStyle(colour)
        } else {
            let big = diameter * 0.3
            HStack(alignment: .firstTextBaseline, spacing: 1) {
                Text(RingRules.centre(ring))
                    .font(.system(size: big, weight: .bold, design: .rounded))
                    .monospacedDigit()
                if ring.value != nil, let unit = ring.unit {
                    Text(unit)
                        .font(.system(size: big * 0.42, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                }
            }
            .lineLimit(1)
            .minimumScaleFactor(0.4)
            .frame(maxWidth: max(room, 1))
        }
    }

    @ViewBuilder private var words: some View {
        VStack(spacing: 6) {
            if ring.label != nil || ring.hint != nil {
                HStack(alignment: .firstTextBaseline, spacing: 4) {
                    if let label = ring.label {
                        Text(label)
                            .font(ring.prominence == .high ? .headline : .subheadline.weight(.semibold))
                            .accessibilityHidden(true)
                    }
                    if let hint = ring.hint { HintButton(hint: hint) }
                }
            }
            if let caption = ring.caption {
                Text(caption).font(.footnote).foregroundStyle(.secondary).accessibilityHidden(true)
            }
            if ring.status != nil || ring.trend != nil {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 10) { statusAndTrend }
                    VStack(spacing: 6) { statusAndTrend }
                }
                .font(.subheadline)
                .accessibilityHidden(true)
            }
        }
        .multilineTextAlignment(.center)
    }

    @ViewBuilder private var statusAndTrend: some View {
        if let status = ring.status { StatusCapsule(badge: status) }
        if let trend = ring.trend { TrendLabel(trend: trend) }
    }
}

/// Concentric rings, the first outside, drawn as the system's activity rings,
/// with the values over their goals beside them in each ring's colour.
struct RingsPart: View {
    let rings: Navree.Rings
    private let scale = TextScale()
    @Environment(\.colorScheme) private var scheme

    private var shown: [Navree.RingValue] { Array(rings.rings.prefix(RingsRules.most)) }

    var body: some View {
        let low = rings.prominence == .low
        let diameter = RingsRules.diameter(rings.prominence, count: shown.count) * scale.capped(low ? 1 : 1.3)
        let dial = Dial(rings: shown, diameter: diameter, prominence: rings.prominence, icon: rings.centre == .icon ? rings.icon : nil)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(rings.title ?? "")
            .accessibilityValue(rings.summary ?? shown.map { "\($0.name) \(RingsRules.value($0))" }.joined(separator: ", "))
        if RingsRules.hasKey(rings) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 24) {
                    dial
                    key(names: rings.legend)
                }
                VStack(spacing: 16) {
                    dial
                    key(names: rings.legend)
                }
            }
        } else {
            dial
        }
    }

    private func key(names: Bool) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(shown.enumerated()), id: \.offset) { _, ring in
                VStack(alignment: .leading, spacing: 0) {
                    if names {
                        Text(ring.name).font(.subheadline.weight(.semibold))
                    }
                    Text(RingsRules.value(ring))
                        .font(.system(.title3, design: .rounded).weight(.semibold))
                        .monospacedDigit()
                        .foregroundStyle(Hues.text(ring.colour, scheme: scheme))
                }
            }
        }
        .fixedSize()
        .accessibilityHidden(true)
    }

    private struct Dial: View {
        let rings: [Navree.RingValue]
        let diameter: CGFloat
        let prominence: Navree.Prominence
        let icon: Navree.Icon?

        var body: some View {
            let stroke = RingsRules.stroke(diameter: diameter, count: rings.count, prominence: prominence)
            ZStack {
                ForEach(Array(rings.enumerated()), id: \.offset) { index, ring in
                    let side = RingsRules.ringDiameter(diameter: diameter, index: index, stroke: stroke) + stroke
                    RingArc(
                        sweep: RingRules.sweep(value: ring.value, max: ring.max),
                        start: Hues.color(ring.colour),
                        end: Hues.end(ring.colour),
                        stroke: stroke
                    )
                    .frame(width: side, height: side)
                }
                if let icon {
                    Glyph(icon: icon).font(.system(size: diameter * 0.16, weight: .semibold)).foregroundStyle(.secondary)
                }
            }
            .frame(width: diameter, height: diameter)
        }
    }
}

/// A square as wide as its place allows, and no wider than `side`: a ring that
/// keeps its size, or shrinks to fit a narrow cell.
struct SquareUpTo: Layout {
    let side: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = min(proposal.width ?? side, side)
        return CGSize(width: width, height: width)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for subview in subviews {
            subview.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
        }
    }
}
