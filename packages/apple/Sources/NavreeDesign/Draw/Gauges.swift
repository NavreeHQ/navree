import NavreeProtocol
import SwiftUI

/// A gauge. Beside a figure's words it is the system's own small circular
/// gauge. Leading a card it is an open arc of the same form, larger, its
/// zones laid along the track in their colours with a hair between them, a
/// marker at the value, the value and the name of its zone in the middle, the
/// scale's ends under the arc's ends, and a key of the zones under it with
/// the one the value is in standing out.
struct GaugePart: View {
    let gauge: Navree.Gauge
    var aside = false

    var body: some View {
        if aside {
            SmallGauge(gauge: gauge)
        } else {
            ZoneGauge(gauge: gauge)
        }
    }
}

/// The system's circular gauge, for a gauge beside other words.
private struct SmallGauge: View {
    let gauge: Navree.Gauge
    private let scale = TextScale()

    var body: some View {
        let side = GaugeRules.diameter(aside: true) * scale.capped(1.3)
        Gauge(value: min(max(gauge.value, gauge.min), gauge.max), in: gauge.min...max(gauge.max, gauge.min + 1)) {
            Text(gauge.label ?? "")
        } currentValueLabel: {
            Text(gauge.shown ?? NumberWords.amount(gauge.value)).fontDesign(.rounded)
        }
        .gaugeStyle(.accessoryCircular)
        .tint(tint)
        .frame(width: side, height: side)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(gauge.label ?? "")
        .accessibilityValue(gauge.summary ?? gauge.shown ?? NumberWords.amount(gauge.value))
    }

    private var tint: AnyShapeStyle {
        if gauge.zones.count > 1 {
            return AnyShapeStyle(Gradient(colors: gauge.zones.map { Hues.color($0.colour) }))
        }
        if let colour = gauge.colour { return AnyShapeStyle(Hues.color(colour)) }
        return AnyShapeStyle(gauge.tone.map(Hues.color) ?? .accentColor)
    }
}

/// The open arc with zones.
private struct ZoneGauge: View {
    let gauge: Navree.Gauge
    private let scale = TextScale()
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let diameter = GaugeRules.diameter(aside: false) * scale.capped(1.25)
        let stroke = diameter * 0.085
        let position = ZoneRules.position(gauge.value, low: gauge.min, high: gauge.max)
        let current = ZoneRules.index(of: gauge.value, in: gauge.zones)
        let markColour = current.map { Hues.color(gauge.zones[$0].colour) }
            ?? gauge.colour.map { Hues.color($0) } ?? Hues.color(gauge.tone)
        VStack(spacing: 10) {
            ZStack {
                track(stroke: stroke)
                Marker(position: position, stroke: stroke)
                    .fill(markColour)
                Marker(position: position, stroke: stroke)
                    .stroke(Surfaces.card, lineWidth: 3)
                VStack(spacing: 2) {
                    Text(gauge.shown ?? NumberWords.amount(gauge.value))
                        .font(.system(size: diameter * 0.2, weight: .bold, design: .rounded))
                        .monospacedDigit()
                        .lineLimit(1)
                        .minimumScaleFactor(0.5)
                    if let current, let label = gauge.zones[current].label {
                        Text(label)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(Hues.text(gauge.zones[current].colour, scheme: scheme))
                            .lineLimit(1)
                            .minimumScaleFactor(0.7)
                    }
                }
                .frame(maxWidth: diameter - 2 * stroke - 24)
                .offset(y: diameter * 0.08)
                ends(diameter: diameter, stroke: stroke)
            }
            .frame(width: diameter, height: diameter * GaugeRules.heightShare, alignment: .top)
            .frame(width: diameter, height: diameter * GaugeRules.heightShare)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(gauge.label ?? "")
            .accessibilityValue(gauge.summary ?? gauge.shown ?? NumberWords.amount(gauge.value))
            VStack(spacing: 4) {
                if gauge.label != nil || gauge.hint != nil {
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        if let label = gauge.label { Text(label).font(.subheadline.weight(.semibold)) }
                        if let hint = gauge.hint { HintButton(hint: hint) }
                    }
                }
                if let caption = gauge.caption {
                    Text(caption).font(.footnote).foregroundStyle(.secondary)
                }
            }
            .multilineTextAlignment(.center)
            if gauge.zones.contains(where: { $0.label != nil }) {
                KeyFlow(
                    entries: gauge.zones.map { (Hues.color($0.colour), $0.label ?? "", nil as String?) },
                    current: current
                )
                .frame(maxWidth: diameter * 1.6)
                .accessibilityHidden(true)
            }
        }
    }

    @ViewBuilder private func track(stroke: CGFloat) -> some View {
        let span = gauge.max - gauge.min
        if gauge.zones.isEmpty || span <= 0 {
            let colour = gauge.colour.map { Hues.color($0) } ?? Hues.color(gauge.tone)
            OpenArc(from: 0, to: 1).stroke(colour.track(scheme), style: StrokeStyle(lineWidth: stroke, lineCap: .round))
            OpenArc(from: 0, to: ZoneRules.position(gauge.value, low: gauge.min, high: gauge.max))
                .stroke(colour, style: StrokeStyle(lineWidth: stroke, lineCap: .round))
        } else {
            // A hair between zones, as a share of the whole arc.
            let hair = 0.008
            ForEach(Array(gauge.zones.enumerated()), id: \.offset) { index, zone in
                let from = ZoneRules.position(min(zone.from, zone.to), low: gauge.min, high: gauge.max)
                let to = ZoneRules.position(max(zone.from, zone.to), low: gauge.min, high: gauge.max)
                OpenArc(from: index == 0 ? from : from + hair, to: index == gauge.zones.count - 1 ? to : to - hair)
                    .stroke(Hues.color(zone.colour), style: StrokeStyle(lineWidth: stroke, lineCap: .butt))
            }
        }
    }

    private func ends(diameter: CGFloat, stroke: CGFloat) -> some View {
        GeometryReader { proxy in
            let radius = diameter / 2 - stroke / 2
            let centre = CGPoint(x: proxy.size.width / 2, y: diameter / 2)
            let low = GaugeRules.angle(0) * .pi / 180
            let high = GaugeRules.angle(1) * .pi / 180
            Text(NumberWords.amount(gauge.min))
                .position(x: centre.x + radius * cos(low), y: centre.y + radius * sin(low) + stroke / 2 + 10)
            Text(NumberWords.amount(gauge.max))
                .position(x: centre.x + radius * cos(high), y: centre.y + radius * sin(high) + stroke / 2 + 10)
        }
        .font(.caption.monospacedDigit())
        .foregroundStyle(.secondary)
    }
}

/// A stretch of the gauge's open arc, as places along it from 0 to 1.
struct OpenArc: Shape {
    var from: Double
    var to: Double

    func path(in rect: CGRect) -> Path {
        let side = rect.width
        let stroke = side * 0.085
        var path = Path()
        path.addArc(
            center: CGPoint(x: rect.midX, y: rect.minY + side / 2),
            radius: side / 2 - stroke / 2,
            startAngle: .degrees(GaugeRules.angle(from)),
            endAngle: .degrees(GaugeRules.angle(max(from, to))),
            clockwise: false
        )
        return path
    }
}

/// The marker at a place along the gauge's arc.
private struct Marker: Shape {
    var position: Double
    var stroke: CGFloat

    func path(in rect: CGRect) -> Path {
        let side = rect.width
        let radius = side / 2 - stroke / 2
        let angle = GaugeRules.angle(position) * .pi / 180
        let centre = CGPoint(x: rect.midX + radius * cos(angle), y: rect.minY + side / 2 + radius * sin(angle))
        let size = stroke + 6
        return Path(ellipseIn: CGRect(x: centre.x - size / 2, y: centre.y - size / 2, width: size, height: size))
    }
}

/// Where one value sits between two ends: its title and value over a line,
/// the line holding its zones as bands with their names under them, a marker
/// at the value, and the page's words (or the numbers) under the two ends.
struct RangePart: View {
    let range: Navree.RangeScale
    @ScaledMetric(relativeTo: .body) private var height: CGFloat = 8
    @Environment(\.colorScheme) private var scheme

    private func amount(_ value: Double) -> String {
        NumberWords.withUnit(NumberWords.amount(value), unit: range.unit)
    }

    var body: some View {
        let current = ZoneRules.index(of: range.value, in: range.zones)
        let markColour = current.map { Hues.color(range.zones[$0].colour) } ?? Color.primary
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                HStack(alignment: .firstTextBaseline, spacing: 4) {
                    if let title = range.title { Text(title).font(.subheadline.weight(.semibold)) }
                    if let hint = range.hint { HintButton(hint: hint) }
                }
                Spacer(minLength: 8)
                HStack(alignment: .firstTextBaseline, spacing: 3) {
                    if let value = range.value {
                        Text(range.shown ?? NumberWords.amount(value))
                            .font(.system(.title3, design: .rounded).weight(.semibold))
                            .monospacedDigit()
                        if let unit = range.unit {
                            Text(unit).font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
                        }
                    } else {
                        Text("–").font(.system(.title3, design: .rounded).weight(.semibold))
                    }
                }
            }
            Line(range: range, height: height, markColour: markColour)
                .frame(height: max(height + 10, 18))
            ends(current: current)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(range.title ?? "")
        .accessibilityValue(range.summary ?? range.shown ?? range.value.map(amount) ?? "")
    }

    private func ends(current: Int?) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(range.low ?? amount(range.min))
            Spacer(minLength: 8)
            if let current, let label = range.zones[current].label {
                Text(label).foregroundStyle(Hues.text(range.zones[current].colour, scheme: scheme)).fontWeight(.semibold)
                Spacer(minLength: 8)
            } else if let label = range.zones.first?.label {
                Text(label).foregroundStyle(.secondary)
                Spacer(minLength: 8)
            }
            Text(range.high ?? amount(range.max))
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .lineLimit(1)
    }

    private struct Line: View {
        let range: Navree.RangeScale
        let height: CGFloat
        let markColour: Color
        @Environment(\.colorScheme) private var scheme

        var body: some View {
            GeometryReader { proxy in
                let width = proxy.size.width
                let middle = proxy.size.height / 2
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.secondary.opacity(scheme == .dark ? 0.3 : 0.18))
                        .frame(height: height)
                        .position(x: width / 2, y: middle)
                    ForEach(Array(range.zones.enumerated()), id: \.offset) { _, zone in
                        let from = ZoneRules.position(min(zone.from, zone.to), low: range.min, high: range.max)
                        let to = ZoneRules.position(max(zone.from, zone.to), low: range.min, high: range.max)
                        Capsule().fill(Hues.color(zone.colour).opacity(0.55))
                            .frame(width: max((to - from) * width, height), height: height)
                            .position(x: (from + to) / 2 * width, y: middle)
                    }
                    if let value = range.value {
                        let x = ZoneRules.position(value, low: range.min, high: range.max) * width
                        let size = height + 10
                        Circle()
                            .fill(markColour)
                            .overlay(Circle().stroke(Surfaces.card, lineWidth: 3))
                            .frame(width: size, height: size)
                            .position(x: min(max(x, size / 2), width - size / 2), y: middle)
                    }
                }
            }
        }
    }
}
