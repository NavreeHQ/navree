import Charts
import NavreeProtocol
import SwiftUI

/// A chart drawn with Swift Charts from the numbers the server sent: its marks
/// are read by VoiceOver and Audio Graphs come with them.
struct ChartPart: View {
    let chart: Navree.Chart

    struct Point: Identifiable {
        let id: String
        let label: String
        let series: String
        let value: Double
    }

    private var points: [Point] {
        chart.series.flatMap { series in
            series.values.enumerated().compactMap { index, value -> Point? in
                guard let value, index < chart.labels.count else { return nil }
                return Point(id: "\(series.label)-\(index)", label: chart.labels[index], series: series.label, value: value)
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title = chart.title {
                Text(title).font(.headline)
            }
            plot
                .frame(minHeight: chart.kind == .sparkline ? 44 : 180)
                .accessibilityLabel(chart.title ?? "Chart")
        }
    }

    @ViewBuilder private var plot: some View {
        let points = self.points
        let many = chart.series.count > 1
        switch chart.kind {
        case .donut:
            Chart(points) { point in
                SectorMark(angle: .value(point.series, point.value), innerRadius: .ratio(0.6), angularInset: 1)
                    .foregroundStyle(by: .value("Label", point.label))
            }
        case .bar, .funnel:
            Chart(points) { point in
                BarMark(x: .value("Label", point.label), y: .value("Value", point.value))
                    .foregroundStyle(by: .value("Series", many ? point.series : ""))
            }
            .chartLegend(many ? .visible : .hidden)
        case .area:
            Chart(points) { point in
                AreaMark(x: .value("Label", point.label), y: .value("Value", point.value))
                    .foregroundStyle(by: .value("Series", many ? point.series : ""))
            }
            .chartLegend(many ? .visible : .hidden)
        case .sparkline:
            Chart(points) { point in
                LineMark(x: .value("Label", point.label), y: .value("Value", point.value))
            }
            .chartXAxis(.hidden)
            .chartYAxis(.hidden)
        case .line:
            Chart(points) { point in
                LineMark(x: .value("Label", point.label), y: .value("Value", point.value))
                    .foregroundStyle(by: .value("Series", many ? point.series : ""))
            }
            .chartLegend(many ? .visible : .hidden)
        }
    }
}
