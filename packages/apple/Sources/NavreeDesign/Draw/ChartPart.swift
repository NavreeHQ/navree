import Charts
import NavreeProtocol
import SwiftUI

/// A chart drawn with Swift Charts from the numbers the server sent: each
/// series in its colour, a gap left a gap, the page's reference lines dashed
/// with their words, its zones as bands, its long axes thinned to a handful
/// of the page's own words. A chart of levels (a night's stages) is drawn in
/// lanes, a stretch of one level a bar in its lane, as the system draws a
/// night. To VoiceOver the chart is one element that says the page's summary.
struct ChartPart: View {
    let chart: Navree.Chart
    /// Whether it stands under a figure's number: smaller, with no heading
    /// and no value axis.
    var small = false
    /// Plots grow with the text, to a point: past it they would push the
    /// numbers they show off the screen.
    private let scale = TextScale()
    private var plotHeight: CGFloat { 180 * scale.capped(1.4) }
    private var smallHeight: CGFloat { 84 * scale.capped(1.4) }
    private var sparkHeight: CGFloat { 36 * scale.capped(1.4) }
    private var laneHeight: CGFloat { 34 * scale.capped(1.4) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !small, chart.kind != .sparkline {
                HStack(alignment: .firstTextBaseline) {
                    ChartHeading(title: chart.title, hint: chart.hint)
                    Spacer(minLength: 8)
                    if let unit = chart.y?.unit, chart.levels.isEmpty, chart.kind != .donut {
                        Text(unit).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            GeometryReader { proxy in
                plot(width: proxy.size.width)
            }
            .frame(height: height)
            if many, chart.kind == .line || chart.kind == .area, chart.levels.isEmpty {
                KeyFlow(entries: chart.series.indices.map { (colour($0), chart.series[$0].label, nil as String?) })
            }
            // The reference lines' words stand under the plot, where no mark
            // can cover them.
            let named = chart.references.filter { $0.label != nil && $0.kind == .level }
            let bands = chart.zones.filter { $0.label != nil }
            if !named.isEmpty || !bands.isEmpty {
                Flow(spacing: 14, lineSpacing: 4) {
                    ForEach(Array(bands.enumerated()), id: \.offset) { _, zone in
                        HStack(spacing: 5) {
                            RoundedRectangle(cornerRadius: 2).fill(Hues.color(zone.colour, fallback: .secondary).opacity(0.3))
                                .frame(width: 12, height: 10)
                            Text(zone.label ?? "")
                        }
                    }
                    ForEach(Array(named.enumerated()), id: \.offset) { _, reference in
                        HStack(spacing: 5) {
                            Dash().stroke(Hues.color(reference.colour, fallback: .secondary), style: StrokeStyle(lineWidth: 1.5, dash: [3, 2]))
                                .frame(width: 16, height: 2)
                            Text(reference.label ?? "")
                        }
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(chart.title ?? "")
        .accessibilityValue(chart.summary ?? sentence)
    }

    private var height: CGFloat {
        if chart.kind == .sparkline { return sparkHeight }
        if !chart.levels.isEmpty { return laneHeight * CGFloat(chart.levels.count) + 24 }
        return small ? smallHeight : plotHeight
    }

    struct Point: Identifiable {
        var id: String { "\(series)-\(index)" }
        let index: Int
        let series: String
        let seriesIndex: Int
        let value: Double
        let run: Int
    }

    private var points: [Point] {
        chart.series.enumerated().flatMap { seriesIndex, series in
            let runs = AxisRules.runs(series.values)
            return series.values.enumerated().compactMap { index, value -> Point? in
                guard let value else { return nil }
                return Point(index: index, series: series.label, seriesIndex: seriesIndex, value: value, run: runs[index])
            }
        }
    }

    private var count: Int {
        max(chart.labels.count, chart.series.map(\.values.count).max() ?? 0)
    }

    private func colour(_ index: Int) -> Color {
        let fallback = HueRules.series[index % HueRules.series.count].color
        return index < chart.series.count ? Hues.color(chart.series[index].colour, fallback: fallback) : fallback
    }

    private var many: Bool { chart.series.count > 1 }

    @ViewBuilder private func plot(width: CGFloat) -> some View {
        if !chart.levels.isEmpty {
            lanes(width: width)
        } else {
            switch chart.kind {
            case .sparkline: sparkline
            case .donut: donut
            case .bar, .funnel: bars(width: width)
            case .line, .area: lines(width: width)
            }
        }
    }

    // MARK: Axes

    private func ticks(width: CGFloat) -> [Int] {
        let longest = chart.labels.map(\.count).max() ?? 4
        // The plot is the chart less the values' axis at its end.
        let most = small ? 3 : AxisRules.most(width: Double(width) - 40, label: Double(longest) * 6.5)
        return AxisRules.ticks(count: count, most: most)
    }

    private func xAxis(width: CGFloat) -> some AxisContent {
        // Bars are named under their middle; a line's times start at their place.
        let bars = chart.kind == .bar || chart.kind == .funnel
        let marks = ticks(width: width)
        let last = marks.last
        return AxisMarks(values: marks.map { String($0) }) { value in
            let index = value.as(String.self).flatMap { Int($0) }
            AxisValueLabel(
                anchor: bars ? .top : index == 0 ? .topLeading : index == last && index == count - 1 ? .topTrailing : .top,
                collisionResolution: .disabled
            ) {
                if let key = value.as(String.self), let index = Int(key), index < chart.labels.count {
                    Text(chart.labels[index])
                }
            }
        }
    }

    private var yAxis: some AxisContent {
        AxisMarks(position: .trailing, values: .automatic(desiredCount: 4)) { _ in
            AxisGridLine()
            AxisValueLabel()
        }
    }

    private var domain: [String] { (0..<count).map { String($0) } }

    // MARK: Marks shared by bars and lines

    @ChartContentBuilder private var guides: some ChartContent {
        ForEach(Array(chart.zones.enumerated()), id: \.offset) { _, zone in
            RectangleMark(yStart: .value("Value", min(zone.from, zone.to)), yEnd: .value("Value", max(zone.from, zone.to)))
                .foregroundStyle(Hues.color(zone.colour, fallback: .secondary).opacity(0.14))
        }
        ForEach(Array(chart.references.enumerated()), id: \.offset) { _, reference in
            if let value = reference.value, reference.kind == .level {
                RuleMark(y: .value("Value", value))
                    .foregroundStyle(Hues.color(reference.colour, fallback: .secondary))
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
            }
        }
    }

    // MARK: Kinds

    private func bars(width: CGFloat) -> some View {
        let bounds = AxisRules.values(chart)
        let horizontal = chart.horizontal || chart.kind == .funnel
        return Chart {
            guides
            ForEach(points) { point in
                if horizontal {
                    BarMark(
                        x: .value("Value", point.value),
                        y: .value("Label", String(point.index)),
                        height: .ratio(0.62),
                        stacking: chart.stacking == .shares ? .normalized : .standard
                    )
                    .foregroundStyle(by: .value("Series", point.series))
                    .cornerRadius(3)
                } else if chart.stacking == .apart && many {
                    BarMark(x: .value("Label", String(point.index)), y: .value("Value", point.value))
                        .foregroundStyle(by: .value("Series", point.series))
                        .position(by: .value("Series", point.series))
                        .cornerRadius(3)
                } else {
                    BarMark(
                        x: .value("Label", String(point.index)),
                        y: .value("Value", point.value),
                        stacking: chart.stacking == .shares ? .normalized : .standard
                    )
                    .foregroundStyle(by: .value("Series", point.series))
                    .cornerRadius(3)
                }
            }
        }
        .chartForegroundStyleScale(domain: chart.series.map(\.label), range: chart.series.indices.map(colour))
        .chartLegend(many ? .visible : .hidden)
        .modifier(BarAxes(horizontal: horizontal, chart: chart, domain: domain, bounds: bounds, small: small, xAxis: xAxis(width: width), yAxis: yAxis))
    }

    private func lines(width: CGFloat) -> some View {
        let bounds = AxisRules.values(chart)
        let curve: InterpolationMethod = chart.curve == .step ? .stepCenter : chart.curve == .smooth ? .monotone : .linear
        return Chart {
            guides
            ForEach(points) { point in
                if chart.kind == .area {
                    AreaMark(
                        x: .value("Label", String(point.index)),
                        yStart: .value("Value", bounds.low),
                        yEnd: .value("Value", point.value),
                        series: .value("Run", "\(point.series)-\(point.run)")
                    )
                    .interpolationMethod(curve)
                    .foregroundStyle(
                        LinearGradient(
                            colors: [colour(point.seriesIndex).opacity(0.32), colour(point.seriesIndex).opacity(0.04)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )
                }
                LineMark(
                    x: .value("Label", String(point.index)),
                    y: .value("Value", point.value),
                    series: .value("Run", "\(point.series)-\(point.run)")
                )
                .interpolationMethod(curve)
                .foregroundStyle(colour(point.seriesIndex))
                .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
            }
        }
        .chartXScale(domain: domain)
        .chartYScale(domain: bounds.low...bounds.high)
        .chartXAxis { xAxis(width: width) }
        .chartYAxis {
            if small { AxisMarks(values: [Double]()) { _ in } } else { yAxis }
        }
        .chartLegend(.hidden)
    }

    private var sparkline: some View {
        let values = points.map(\.value)
        let low = values.min() ?? 0
        let high = values.max() ?? 1
        let pad = high > low ? (high - low) * 0.12 : 1
        return Chart {
            ForEach(points) { point in
                AreaMark(
                    x: .value("Label", String(point.index)),
                    yStart: .value("Value", low - pad),
                    yEnd: .value("Value", point.value),
                    series: .value("Run", "\(point.series)-\(point.run)")
                )
                .interpolationMethod(.monotone)
                .foregroundStyle(
                    LinearGradient(colors: [colour(point.seriesIndex).opacity(0.25), colour(point.seriesIndex).opacity(0)], startPoint: .top, endPoint: .bottom)
                )
                LineMark(
                    x: .value("Label", String(point.index)),
                    y: .value("Value", point.value),
                    series: .value("Run", "\(point.series)-\(point.run)")
                )
                .interpolationMethod(.monotone)
                .foregroundStyle(colour(point.seriesIndex))
                .lineStyle(StrokeStyle(lineWidth: 1.75, lineCap: .round))
            }
            if let last = points.last {
                PointMark(x: .value("Label", String(last.index)), y: .value("Value", last.value))
                    .foregroundStyle(colour(last.seriesIndex))
                    .symbolSize(22)
            }
        }
        .chartXScale(domain: domain)
        .chartYScale(domain: (low - pad)...(high + pad))
        .chartXAxis(.hidden)
        .chartYAxis(.hidden)
        .chartLegend(.hidden)
    }

    private var donut: some View {
        let labelled = chart.series.first.map { series in
            series.values.enumerated().compactMap { index, value -> (String, Double)? in
                guard let value, index < chart.labels.count else { return nil }
                return (chart.labels[index], value)
            }
        } ?? []
        return Chart(Array(labelled.enumerated()), id: \.offset) { index, item in
            SectorMark(angle: .value("Value", item.1), innerRadius: .ratio(0.62), angularInset: 1.5)
                .cornerRadius(3)
                .foregroundStyle(by: .value("Label", item.0))
        }
        .chartForegroundStyleScale(domain: labelled.map(\.0), range: labelled.indices.map { HueRules.series[$0 % HueRules.series.count].color })
    }

    /// The stages of a night: a lane a level, the first at the foot, each run
    /// of one level a bar in its lane with a hairline down to the next, the
    /// names of the lanes inside the plot so it has the whole width (and
    /// stands over a strip of the same night edge to edge).
    private func lanes(width: CGFloat) -> some View {
        let series = chart.series.first
        let stretches = AxisRules.stretches(series?.values ?? [])
        let hue = colour(0)
        let top = Double(chart.levels.count) - 0.25
        return Chart {
            ForEach(Array(chart.levels.indices), id: \.self) { level in
                RuleMark(y: .value("Level", Double(level)))
                    .foregroundStyle(Color.secondary.opacity(0.18))
                    .lineStyle(StrokeStyle(lineWidth: 1))
            }
            ForEach(Array(stretches.enumerated()), id: \.offset) { index, stretch in
                RectangleMark(
                    xStart: .value("Step", Double(stretch.from)),
                    xEnd: .value("Step", Double(stretch.to + 1)),
                    yStart: .value("Level", Double(stretch.level) - 0.26),
                    yEnd: .value("Level", Double(stretch.level) + 0.26)
                )
                .foregroundStyle(hue.opacity(0.45 + 0.55 * (1 - Double(stretch.level) / max(Double(chart.levels.count - 1), 1))))
                .cornerRadius(3)
                if index + 1 < stretches.count, stretches[index + 1].from == stretch.to + 1 {
                    let next = stretches[index + 1]
                    RuleMark(
                        x: .value("Step", Double(next.from)),
                        yStart: .value("Level", Double(min(stretch.level, next.level)) + 0.26),
                        yEnd: .value("Level", Double(max(stretch.level, next.level)) - 0.26)
                    )
                    .foregroundStyle(hue.opacity(0.5))
                    .lineStyle(StrokeStyle(lineWidth: 1))
                }
            }
        }
        .chartXScale(domain: 0...Double(max(count, 1)))
        .chartYScale(domain: -0.4...top)
        .chartYAxis(.hidden)
        .chartXAxis {
            AxisMarks(values: ticks(width: width).map { Double($0) }) { value in
                AxisValueLabel(anchor: .topLeading, collisionResolution: .disabled) {
                    if let place = value.as(Double.self), Int(place) < chart.labels.count {
                        Text(chart.labels[Int(place)])
                    }
                }
            }
        }
        .chartOverlay { proxy in
            GeometryReader { geometry in
                if let frame = proxy.plotFrame.map({ geometry[$0] }) {
                    ForEach(Array(chart.levels.enumerated()), id: \.offset) { level, name in
                        if let y = proxy.position(forY: Double(level) + 0.5) {
                            Text(name)
                                .font(.caption2.weight(.medium))
                                .foregroundStyle(.secondary)
                                .fixedSize()
                                .frame(width: frame.width, alignment: .leading)
                                .position(x: frame.minX + frame.width / 2, y: frame.minY + y)
                        }
                    }
                }
            }
        }
    }

    private var sentence: String {
        chart.series.map { series in
            let values = series.values.enumerated().compactMap { index, value -> String? in
                guard let value else { return nil }
                let label = index < chart.labels.count ? chart.labels[index] : String(index + 1)
                let words = index < series.shown.count ? series.shown[index] : NumberWords.amount(value)
                return "\(label) \(words)"
            }
            return "\(series.label): " + values.joined(separator: ", ")
        }.joined(separator: ". ")
    }
}

/// The axes of a bar chart, upright or lying down.
private struct BarAxes<X: AxisContent, Y: AxisContent>: ViewModifier {
    let horizontal: Bool
    let chart: Navree.Chart
    let domain: [String]
    let bounds: AxisRules.Bounds
    let small: Bool
    let xAxis: X
    let yAxis: Y

    func body(content: Content) -> some View {
        if horizontal {
            content
                .chartYScale(domain: domain)
                .chartYAxis {
                    AxisMarks(position: .leading, values: domain) { value in
                        AxisValueLabel {
                            if let key = value.as(String.self), let index = Int(key), index < chart.labels.count {
                                Text(chart.labels[index])
                            }
                        }
                    }
                }
        } else {
            content
                .chartXScale(domain: domain)
                .chartYScale(domain: bounds.low...bounds.high)
                .chartXAxis { xAxis }
                .chartYAxis {
                    if small { AxisMarks(values: [Double]()) { _ in } } else { yAxis }
                }
        }
    }
}

/// A short level line, for a key.
private struct Dash: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.minX, y: rect.midY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
        return path
    }
}
