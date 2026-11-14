import Charts
import NavreeProtocol
import SwiftUI

/// The calendar of days: a week a column and a weekday a row, from the
/// person's own first weekday, the months named along the top in the
/// person's language, four shades of one colour from the page's steps, and
/// no shade at all for a day with no reading (no data is not a day of none).
struct CalendarHeatmapPart: View {
    let heatmap: Navree.CalendarHeatmap
    @Environment(\.calendar) private var personCalendar
    @Environment(\.locale) private var locale
    @Environment(\.colorScheme) private var scheme
    @ScaledMetric(relativeTo: .caption2) private var labelSize: CGFloat = 11

    private var calendar: Calendar {
        var calendar = personCalendar
        calendar.timeZone = TimeZone(identifier: "UTC")!
        calendar.locale = locale
        return calendar
    }

    var body: some View {
        let days = heatmap.days.compactMap { day in PagerRules.date(day.date).map { (date: $0, value: day.value) } }
        let first = heatmap.first.flatMap(PagerRules.date) ?? days.map(\.date).min() ?? Date()
        let last = heatmap.last.flatMap(PagerRules.date) ?? days.map(\.date).max() ?? first
        let weeks = max(HeatRules.weeks(first: first, last: last, calendar: calendar), 1)
        let highest = days.compactMap(\.value).max() ?? 1
        let colour = Hues.color(heatmap.colour, fallback: .accentColor, scheme: scheme)
        VStack(alignment: .leading, spacing: 10) {
            ChartHeading(title: heatmap.title, hint: heatmap.hint)
            let label = labelSize
            HeightFromWidth(height: { width in
                let step = min((width - label * 1.6) / CGFloat(weeks), 20)
                return label + 6 + step * 7
            }) {
                Canvas { context, size in
                    // The names of the weekdays in a gutter, the months along
                    // the top, a square a day under them.
                    let gutter = label * 1.6
                    let step = min((size.width - gutter) / CGFloat(weeks), 20)
                    let cell = max(step - 2, 2)
                    let top = label + 6
                    let start = HeatRules.start(first, calendar: calendar)
                    for mark in HeatRules.months(first: first, weeks: weeks, calendar: calendar) {
                        let name = Text(calendar.shortMonthSymbols[mark.month - 1]).font(.system(size: label)).foregroundStyle(.secondary)
                        context.draw(name, at: CGPoint(x: gutter + CGFloat(mark.week) * step, y: 0), anchor: .topLeading)
                    }
                    for row in [1, 3, 5] {
                        let name = Text(weekday(row)).font(.system(size: label)).foregroundStyle(.secondary)
                        context.draw(name, at: CGPoint(x: 0, y: top + CGFloat(row) * step + cell / 2), anchor: .leading)
                    }
                    for day in days {
                        let place = HeatRules.cell(day.date, start: start, calendar: calendar)
                        let rect = CGRect(x: gutter + CGFloat(place.week) * step, y: top + CGFloat(place.day) * step, width: cell, height: cell)
                        let shape = Path(roundedRect: rect, cornerRadius: cell * 0.22)
                        if let shade = HeatRules.shade(day.value, steps: heatmap.steps, highest: highest) {
                            context.fill(shape, with: .color(colour.opacity(HeatRules.opacities[shade])))
                        } else {
                            context.stroke(shape, with: .color(.secondary.opacity(0.3)), lineWidth: 0.75)
                        }
                    }
                }
            }
            legend(colour: colour, highest: highest)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(heatmap.title ?? "")
        .accessibilityValue(SpeechRules.value(heatmap.summary ?? "", after: heatmap.title))
    }

    private func weekday(_ row: Int) -> String {
        let symbols = calendar.veryShortStandaloneWeekdaySymbols
        return symbols[(calendar.firstWeekday - 1 + row) % 7]
    }

    private func legend(colour: Color, highest: Double) -> some View {
        let bounds = heatmap.steps.count >= 3 ? Array(heatmap.steps.prefix(3)) : (1...3).map { highest * Double($0) / 4 }
        let words = ["0"] + bounds.map { NumberWords.amount($0) }
        return HStack(spacing: 12) {
            if let label = heatmap.label {
                Text(label).foregroundStyle(.secondary)
            }
            ForEach(0..<4, id: \.self) { shade in
                HStack(spacing: 4) {
                    RoundedRectangle(cornerRadius: 2.5).fill(colour.opacity(HeatRules.opacities[shade])).frame(width: 11, height: 11)
                    Text(shade == 3 ? "\(words[shade])+" : words[shade]).monospacedDigit()
                }
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }
}

/// Every measure against every other: a cell a pair, in one colour for
/// measures that rise together and another for those that pull apart, deeper
/// the stronger; a pair that may be chance is grey, one never tested blank.
struct MatrixHeatmapPart: View {
    let matrix: Navree.MatrixHeatmap
    @ScaledMetric(relativeTo: .footnote) private var cellSize: CGFloat = 44

    var body: some View {
        let measures = matrix.measures
        let rows = matrix.lower ? Array(measures.indices.dropFirst()) : Array(measures.indices)
        let columns = matrix.lower ? Array(measures.indices.dropLast()) : Array(measures.indices)
        VStack(alignment: .leading, spacing: 10) {
            ChartHeading(title: matrix.title, hint: matrix.hint)
            Grid(horizontalSpacing: 3, verticalSpacing: 3) {
                ForEach(rows, id: \.self) { row in
                    GridRow {
                        Text(measures[row]).font(.footnote).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
                        ForEach(columns, id: \.self) { column in
                            if MatrixRules.drawn(row: row, column: column, lower: matrix.lower) {
                                cell(row: row, column: column)
                            } else {
                                Color.clear.frame(width: cellSize, height: cellSize)
                            }
                        }
                    }
                }
                GridRow {
                    Color.clear.frame(width: 1, height: 1)
                    ForEach(columns, id: \.self) { column in
                        Text(measures[column]).font(.footnote).foregroundStyle(.secondary)
                            .lineLimit(1).minimumScaleFactor(0.6).frame(width: cellSize)
                    }
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(matrix.title ?? "")
        .accessibilityValue(SpeechRules.value(matrix.summary ?? "", after: matrix.title))
    }

    private func cell(row: Int, column: Int) -> some View {
        let relation = MatrixRules.relation(matrix.relations, row, column)
        let strength = MatrixRules.strength(relation, peak: matrix.peak)
        let value = relation?.value ?? 0
        let colour: Color
        if relation?.value == nil {
            colour = .clear
        } else if relation?.significant == false {
            colour = Color.secondary.opacity(0.25)
        } else {
            colour = Hues.color(value >= 0 ? matrix.positive : matrix.negative, fallback: value >= 0 ? .blue : .orange)
                .opacity(0.15 + 0.85 * strength)
        }
        return RoundedRectangle(cornerRadius: 6, style: .continuous)
            .fill(colour)
            .overlay {
                if relation?.value == nil {
                    RoundedRectangle(cornerRadius: 6, style: .continuous).stroke(Color.secondary.opacity(0.3), lineWidth: 0.75)
                } else {
                    Text(String(format: "%.2f", value))
                        .font(.caption2.weight(.semibold))
                        .monospacedDigit()
                        .foregroundStyle(strength > 0.55 ? Color.white : Color.primary)
                }
            }
            .frame(width: cellSize, height: cellSize)
    }
}

/// Clouds of points on two numeric axes, each cloud in its colour and seen
/// through, with the page's reference lines; with equal axes the plot is
/// square and a diagonal is the line of no change.
struct ScatterPart: View {
    let scatter: Navree.Scatter

    var body: some View {
        let bounds = AxisRules.scatter(scatter)
        VStack(alignment: .leading, spacing: 10) {
            ChartHeading(title: scatter.title, hint: scatter.hint)
            Chart {
                ForEach(Array(scatter.references.enumerated()), id: \.offset) { _, reference in
                    if reference.kind == .diagonal {
                        LineMark(x: .value("x", bounds.x.low), y: .value("y", bounds.x.low), series: .value("Reference", "diagonal"))
                            .foregroundStyle(Color.secondary)
                            .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                        LineMark(x: .value("x", bounds.x.high), y: .value("y", bounds.x.high), series: .value("Reference", "diagonal"))
                            .foregroundStyle(Color.secondary)
                            .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                    } else if let value = reference.value {
                        RuleMark(y: .value("y", value))
                            .foregroundStyle(Color.secondary)
                            .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                    }
                }
                ForEach(Array(scatter.series.enumerated()), id: \.offset) { index, series in
                    ForEach(Array(series.points.enumerated()), id: \.offset) { _, point in
                        PointMark(x: .value(scatter.x?.name ?? "x", point.x), y: .value(scatter.y?.name ?? "y", point.y))
                            .foregroundStyle(by: .value("Series", series.name))
                            .opacity(0.6)
                    }
                }
            }
            .chartForegroundStyleScale(
                domain: scatter.series.map(\.name),
                range: scatter.series.enumerated().map { Hues.color($0.element.colour, fallback: HueRules.series[$0.offset % HueRules.series.count].color) }
            )
            .chartXScale(domain: bounds.x.low...bounds.x.high)
            .chartYScale(domain: bounds.y.low...bounds.y.high)
            .chartXAxisLabel(axisWords(scatter.x), alignment: .center)
            .chartYAxisLabel(axisWords(scatter.y))
            .chartLegend(scatter.series.count > 1 ? .visible : .hidden)
            .aspectRatio(scatter.equalAxes ? 1 : 1.4, contentMode: .fit)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(scatter.title ?? "")
        .accessibilityValue(SpeechRules.value(scatter.summary ?? "", after: scatter.title))
    }

    private func axisWords(_ measure: Navree.Measure?) -> String {
        guard let measure else { return "" }
        return [measure.name, measure.unit.map { "(\($0))" }].compactMap { $0 }.joined(separator: " ")
    }
}

/// One part whose height follows from the width it is given: a chart of
/// square cells, as wide as its place.
struct HeightFromWidth: Layout {
    let height: @Sendable (CGFloat) -> CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 320
        return CGSize(width: width, height: height(width))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for subview in subviews {
            subview.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
        }
    }
}
