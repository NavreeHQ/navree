import NavreeProtocol
import SwiftUI

/// A meter: one bar of segments that add up to the whole, each in its colour
/// with a hair between them and its share written inside where it fits; under
/// it, each segment's amount in the page's words and what the page says of it.
struct MeterPart: View {
    let meter: Navree.Meter
    private let scale = TextScale()
    private var height: CGFloat { 26 * scale.capped(1.5) }

    var body: some View {
        let shares = ShareRules.shares(meter.segments.map(\.value))
        VStack(alignment: .leading, spacing: 12) {
            ChartHeading(title: meter.title, hint: meter.hint)
            GeometryReader { proxy in
                let drawn = meter.segments.indices.filter { shares[$0] > 0 }
                let hair: CGFloat = 2
                let room = proxy.size.width - hair * CGFloat(max(drawn.count - 1, 0))
                HStack(spacing: hair) {
                    ForEach(drawn, id: \.self) { index in
                        let colour = Hues.color(meter.segments[index].colour, fallback: HueRules.series[index % HueRules.series.count].color)
                        let width = room * shares[index]
                        let words = "\(ShareRules.percent(shares[index]))%"
                        Rectangle()
                            .fill(colour)
                            .frame(width: width)
                            .overlay {
                                if ShareRules.saysInside(shares[index], width: room, textWidth: Double(words.count) * 7.5) {
                                    Text(words)
                                        .font(.caption.weight(.semibold))
                                        .monospacedDigit()
                                        .foregroundStyle(.white)
                                        .lineLimit(1)
                                }
                            }
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            }
            .frame(height: height)
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                ForEach(Array(meter.segments.enumerated()), id: \.offset) { index, segment in
                    GridRow(alignment: .firstTextBaseline) {
                        Circle()
                            .fill(Hues.color(segment.colour, fallback: HueRules.series[index % HueRules.series.count].color))
                            .frame(width: 9, height: 9)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(segment.name).font(.subheadline)
                            if let detail = segment.detail {
                                Text(detail).font(.footnote).foregroundStyle(.secondary)
                            }
                        }
                        .gridColumnAlignment(.leading)
                        Text(segment.shown ?? segment.value.map { NumberWords.amount($0) } ?? "–")
                            .font(.subheadline.weight(.semibold))
                            .monospacedDigit()
                            .frame(maxWidth: .infinity, alignment: .trailing)
                    }
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(meter.title ?? "")
        .accessibilityValue(SpeechRules.value(meter.summary ?? sentence, after: meter.title))
    }

    private var sentence: String {
        meter.segments.map { segment in
            [segment.name, segment.shown, segment.detail].compactMap { $0 }.joined(separator: " ")
        }.joined(separator: ", ")
    }
}

/// Columns: one an item, each split into the same parts and drawn whole, so
/// they compare what each is made of rather than how big it is. The column
/// that stands out is drawn in full and the others quieter.
struct ColumnsPart: View {
    let columns: Navree.MiniColumns
    private let scale = TextScale()
    private var height: CGFloat { 120 * scale.capped(1.4) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ChartHeading(title: columns.title, hint: columns.hint)
            HStack(alignment: .bottom, spacing: 10) {
                ForEach(Array(columns.columns.enumerated()), id: \.offset) { index, column in
                    let shares = ShareRules.shares(column.values)
                    let quiet = columns.highlight.map { $0 != index } ?? false
                    VStack(spacing: 6) {
                        if let shown = column.shown {
                            Text(shown)
                                .font(.caption2.weight(.semibold))
                                .monospacedDigit()
                                .lineLimit(1)
                                .minimumScaleFactor(0.8)
                        }
                        VStack(spacing: 1.5) {
                            // The first part at the foot.
                            ForEach(Array(shares.enumerated().reversed()), id: \.offset) { part, share in
                                if share > 0 {
                                    Rectangle()
                                        .fill(colour(part))
                                        .frame(height: max(height * share - 1.5, 1))
                                }
                            }
                        }
                        .frame(height: height, alignment: .bottom)
                        .clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
                        .opacity(quiet ? 0.4 : 1)
                        Text(column.label)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .minimumScaleFactor(0.7)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            KeyFlow(entries: columns.parts.enumerated().map { (colour($0.offset), $0.element.name, $0.element.detail) })
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(columns.title ?? "")
        .accessibilityValue(SpeechRules.value(columns.summary ?? sentence, after: columns.title))
    }

    private func colour(_ part: Int) -> Color {
        let fallback = HueRules.series[part % HueRules.series.count].color
        return part < columns.parts.count ? Hues.color(columns.parts[part].colour, fallback: fallback) : fallback
    }

    private var sentence: String {
        columns.columns.map { column in
            let parts = zip(columns.parts, column.values).compactMap { part, value in
                value.map { "\(part.name) \(NumberWords.amount($0))" }
            }
            return ([column.label, column.shown].compactMap { $0 } + parts).joined(separator: " ")
        }.joined(separator: ", ")
    }
}

/// A strip: equal cells that touch, each in the colour of its kind and a quiet
/// cell where there is no reading; the first and last cells' words under its
/// ends, and a key that says each kind's share.
struct StripPart: View {
    let strip: Navree.ColorStrip
    private let scale = TextScale()
    private var height: CGFloat { 28 * scale.capped(1.5) }
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ChartHeading(title: strip.title, hint: strip.hint)
            Canvas { context, size in
                let count = max(strip.cells.count, 1)
                let width = size.width / CGFloat(count)
                for (index, cell) in strip.cells.enumerated() {
                    let colour: Color
                    if let category = cell.category, category >= 0, category < strip.key.count {
                        colour = Hues.color(strip.key[category].colour, fallback: HueRules.series[category % HueRules.series.count].color, scheme: scheme)
                    } else {
                        colour = Color.secondary.opacity(0.15)
                    }
                    // A hair of overlap, so neighbours touch with no seam.
                    let rect = CGRect(x: CGFloat(index) * width, y: 0, width: width + 0.5, height: size.height)
                    context.fill(Path(rect), with: .color(colour))
                }
            }
            .frame(height: height)
            .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            if let first = strip.cells.first, let last = strip.cells.last {
                HStack {
                    Text(first.label)
                    Spacer()
                    if let label = strip.label { Text(label) ; Spacer() }
                    Text(last.label)
                }
                .font(.caption)
                .monospacedDigit()
                .foregroundStyle(.secondary)
            }
            KeyFlow(entries: entries)
                .padding(.top, 2)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(strip.title ?? "")
        .accessibilityValue(SpeechRules.value(strip.summary ?? entries.map { KeyRules.entry($0.name, detail: $0.detail) }.joined(separator: ", "), after: strip.title))
    }

    private var entries: [(color: Color, name: String, detail: String?)] {
        let shares = strip.shares ? ShareRules.categoryShares(strip.cells, keySize: strip.key.count) : []
        return strip.key.enumerated().map { index, entry in
            let detail = entry.detail ?? (index < shares.count ? "\(ShareRules.percent(shares[index]))%" : nil)
            return (Hues.color(entry.colour, fallback: HueRules.series[index % HueRules.series.count].color), entry.name, detail)
        }
    }
}
