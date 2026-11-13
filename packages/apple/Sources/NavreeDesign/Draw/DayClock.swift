import NavreeProtocol
import SwiftUI

/// The day clock: 24 hours round a dial, midnight at the top. Each hour is a
/// bar outwards as long as how much happened in it, in the colour of what the
/// hour was; the quarters are named at the edge in the person's own clock;
/// the day's moments are marked on one line round the bars at their time;
/// the hour that is now has a rim; the day's total is in the middle. Under
/// the dial, the key with each kind's total and the moments with their times.
struct DayClockPart: View {
    let clock: Navree.DayClock
    private let scale = TextScale()
    @Environment(\.locale) private var locale

    var body: some View {
        let uses24 = ClockRules.uses24(locale)
        VStack(spacing: 14) {
            ChartHeading(title: clock.title, hint: clock.hint)
                .frame(maxWidth: .infinity, alignment: .leading)
            Dial(clock: clock, uses24: uses24, scale: scale.capped(1.25))
                .aspectRatio(1, contentMode: .fit)
                .frame(maxWidth: ClockRules.diameter * scale.capped(1.25))
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(clock.title ?? clock.label ?? "")
                .accessibilityValue(clock.summary ?? sentence(uses24: uses24))
            VStack(alignment: .leading, spacing: 10) {
                if !clock.key.isEmpty {
                    KeyFlow(entries: clock.key.map { (Hues.color($0.colour, fallback: .secondary), $0.name, $0.detail) })
                }
                ForEach(Array(clock.moments.enumerated()), id: \.offset) { _, moment in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        if let icon = moment.icon {
                            Glyph(icon: icon).foregroundStyle(Hues.color(moment.colour, fallback: .secondary))
                                .frame(minWidth: 20)
                        }
                        Text(moment.name)
                        Spacer(minLength: 8)
                        Text(ClockRules.timeLabel(moment.at, uses24: uses24, locale: locale))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                    .font(.subheadline)
                }
            }
            .accessibilityHidden(true)
        }
    }

    private func sentence(uses24: Bool) -> String {
        var parts: [String] = []
        if let shown = clock.shown { parts.append([shown, clock.unit].compactMap { $0 }.joined(separator: " ")) }
        parts += clock.key.map { KeyRules.entry($0.name, detail: $0.detail) }
        parts += clock.moments.map { ClockRules.momentEntry($0.name, at: $0.at, uses24: uses24, locale: locale) }
        return parts.joined(separator: ". ")
    }

    private struct Dial: View {
        let clock: Navree.DayClock
        let uses24: Bool
        let scale: CGFloat
        @Environment(\.locale) private var locale
        @Environment(\.colorScheme) private var scheme

        var body: some View {
            GeometryReader { proxy in
                let side = min(proxy.size.width, proxy.size.height)
                let half = side / 2
                let label = 15 * scale
                let mark = 22 * scale
                let dial = ClockRules.dial(half: half, label: label, mark: mark)
                let centre = CGPoint(x: proxy.size.width / 2, y: proxy.size.height / 2)
                ZStack {
                    bars(dial: dial, centre: centre)
                    ForEach(ClockRules.quarters, id: \.self) { hour in
                        Text(ClockRules.quarterLabel(hour, uses24: uses24, locale: locale))
                            .font(.system(size: 11 * scale, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                            .fixedSize()
                            .position(point(hours: Double(hour), radius: dial.labels, centre: centre))
                    }
                    moments(dial: dial, centre: centre, mark: mark)
                    middle(radius: dial.inner)
                        .position(centre)
                }
            }
        }

        private func point(hours: Double, radius: Double, centre: CGPoint) -> CGPoint {
            let angle = ClockRules.angle(hours: hours) * .pi / 180
            return CGPoint(x: centre.x + radius * cos(angle), y: centre.y + radius * sin(angle))
        }

        private func bars(dial: ClockRules.Dial, centre: CGPoint) -> some View {
            let most = ClockRules.most(clock)
            let width = ClockRules.barWidth(inner: dial.inner)
            let length = dial.outer - dial.inner - width
            let stub = 0.06
            return Canvas { context, _ in
                // The dial's face: a faint band where the hours stand.
                let middle = (dial.inner + dial.outer) / 2
                var band = Path()
                band.addEllipse(in: CGRect(x: centre.x - middle, y: centre.y - middle, width: 2 * middle, height: 2 * middle))
                context.stroke(band, with: .color(.secondary.opacity(scheme == .dark ? 0.14 : 0.08)), lineWidth: dial.outer - dial.inner + 4)
                for (index, hour) in clock.hours.prefix(24).enumerated() {
                    let colour = hour.category.flatMap { $0 < clock.key.count ? clock.key[$0].colour : nil }
                    let base = Hues.color(colour ?? clock.colour, fallback: .accentColor, scheme: scheme)
                    let reach = ClockRules.reach(hour, most: most)
                    let start = dial.inner + width / 2
                    let end = start + length * max(reach ?? 0, stub)
                    let angle = ClockRules.angle(hours: Double(index) + 0.5) * .pi / 180
                    var bar = Path()
                    bar.move(to: CGPoint(x: centre.x + start * cos(angle), y: centre.y + start * sin(angle)))
                    bar.addLine(to: CGPoint(x: centre.x + end * cos(angle), y: centre.y + end * sin(angle)))
                    let faint = reach == nil || (reach ?? 0) < stub
                    context.stroke(
                        bar,
                        with: .color(faint ? base.opacity(reach == nil ? 0.18 : 0.4) : base),
                        style: StrokeStyle(lineWidth: width, lineCap: .round)
                    )
                    if clock.now == index {
                        context.stroke(
                            bar,
                            with: .color(.primary),
                            style: StrokeStyle(lineWidth: width + 5, lineCap: .round)
                        )
                        context.stroke(bar, with: .color(base), style: StrokeStyle(lineWidth: width, lineCap: .round))
                    }
                }
            }
        }

        private func moments(dial: ClockRules.Dial, centre: CGPoint, mark: CGFloat) -> some View {
            // Two marks never closer than their own width along the line.
            let gap = Double(mark + 3) / (2 * .pi * dial.moments) * 24
            let placed = ClockRules.spread(clock.moments.map(\.at), gap: gap)
            return ForEach(Array(clock.moments.enumerated()), id: \.offset) { index, moment in
                let colour = Hues.color(moment.colour, fallback: .secondary, scheme: scheme)
                ZStack {
                    Circle().fill(colour)
                    Circle().stroke(Surfaces.card, lineWidth: 2)
                    if let icon = moment.icon {
                        Glyph(icon: icon)
                            .font(.system(size: mark * 0.5, weight: .semibold))
                            .foregroundStyle(.white)
                    }
                }
                .frame(width: mark, height: mark)
                .position(point(hours: placed[index], radius: dial.moments, centre: centre))
            }
        }

        private func middle(radius: Double) -> some View {
            VStack(spacing: 0) {
                if let icon = clock.icon {
                    Glyph(icon: icon)
                        .font(.system(size: 15 * scale, weight: .semibold))
                        .foregroundStyle(Hues.color(clock.colour, fallback: .secondary))
                        .padding(.bottom, 2)
                }
                if let shown = clock.shown {
                    Text(shown)
                        .font(.system(size: 30 * scale, weight: .bold, design: .rounded))
                        .monospacedDigit()
                        .lineLimit(1)
                        .minimumScaleFactor(0.5)
                }
                if let unit = clock.unit ?? clock.label {
                    Text(unit)
                        .font(.system(size: 13 * scale, weight: .medium))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                }
            }
            .frame(width: radius * 1.5)
        }
    }
}
