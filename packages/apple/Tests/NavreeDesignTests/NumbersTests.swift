import Foundation
import NavreeProtocol
import SwiftUI
import Testing

@testable import NavreeDesign

/// Numbers and ranges a server may send, made safe before they reach a
/// control, a chart or a whole number.
@MainActor
struct NumbersTests {
    @Test func aWholeNumberIsMadeFromAnyNumber() {
        #expect(SafeNumbers.int(42.9) == 42)
        #expect(SafeNumbers.int(-42.9) == -42)
        #expect(SafeNumbers.int(.nan) == 0)
        #expect(SafeNumbers.int(.infinity) == Int.max)
        #expect(SafeNumbers.int(-.infinity) == Int.min)
        #expect(SafeNumbers.int(9.3e18) == Int.max)
        #expect(SafeNumbers.int(1e19) == Int.max)
        #expect(SafeNumbers.int(-1e19) == Int.min)
        // The largest whole number, as a number with a fraction, is one past it.
        #expect(SafeNumbers.int(Double(Int.max)) == Int.max)
        #expect(SafeNumbers.int(9.2e18) == 9_200_000_000_000_000_000)
    }

    @Test func aRangeAlwaysHasItsLowEndUnderItsHighEnd() {
        let cases: [(Double, Double)] = [
            (0, 100), (100, 0), (5, 5), (.nan, 10), (10, .nan), (.nan, .nan), (-.infinity, .infinity), (.infinity, -.infinity),
            (1e300, 1e300), (-1e308, 1e308), (0, 0),
        ]
        for (low, high) in cases {
            let range = SafeNumbers.range(low, high)
            #expect(range.lowerBound < range.upperBound, "\(low) to \(high)")
            #expect(range.lowerBound.isFinite && range.upperBound.isFinite, "\(low) to \(high)")
        }
        #expect(SafeNumbers.range(0, 100) == 0...100)
        #expect(SafeNumbers.range(100, 0) == 0...100)
        #expect(SafeNumbers.clamped(.nan, to: 0...10) == 0)
        #expect(SafeNumbers.clamped(1e30, to: 0...10) == 10)
    }

    // MARK: - Charts

    static func line(_ values: [Double], min: Double? = nil, max: Double? = nil) -> Navree.Chart {
        Navree.Chart(
            kind: .line, series: [Navree.Series(label: "Goal", values: values)],
            y: min == nil && max == nil ? nil : Navree.Measure(max: max, min: min)
        )
    }

    @Test func aValueAxisWhoseTopIsUnderItsValuesStillRunsUpwards() {
        // A percent of goal with its top fixed at 100, and a week at 120, 135 and 150.
        let past = AxisRules.values(Self.line([120, 135, 150], max: 100))
        #expect(past.low < past.high)
        #expect(past.high == 100)
        // Ends given the wrong way round, the same, or with only a foot over the values.
        for chart in [
            Self.line([1, 2, 3], min: 10, max: 5), Self.line([1, 2, 3], min: 7, max: 7), Self.line([1, 2, 3], min: 200),
            Self.line([]), Self.line([0, 0, 0]), Self.line([-1e308, 1e308]), Self.line([1e308, 1e308]),
            Self.line([5], min: -1e308, max: 1e308),
        ] {
            let bounds = AxisRules.values(chart)
            #expect(bounds.low < bounds.high, "\(chart.series[0].values) \(String(describing: chart.y))")
            #expect(bounds.low.isFinite && bounds.high.isFinite)
        }
        // An ordinary chart is as it was.
        #expect(AxisRules.values(Self.line([10, 20, 30], min: 0, max: 40)) == AxisRules.Bounds(0, 40))
    }

    @Test func aScattersAxesRunUpwardsWhateverThePageFixes() {
        let points = Navree.PointSeries(name: "Nights", points: [Navree.Point(x: 120, y: 7), Navree.Point(x: 150, y: 9)])
        for (x, y) in [
            (Navree.Measure(max: 100), Navree.Measure(max: 5, min: 10)), (Navree.Measure(min: 500), Navree.Measure()),
            (Navree.Measure(max: 1e308, min: -1e308), Navree.Measure(max: 3, min: 3)),
        ] {
            for equal in [false, true] {
                let bounds = AxisRules.scatter(Navree.Scatter(equalAxes: equal, series: [points], x: x, y: y))
                #expect(bounds.x.low < bounds.x.high && bounds.y.low < bounds.y.high, "\(x) \(y) \(equal)")
                let finite = [bounds.x.low, bounds.x.high, bounds.y.low, bounds.y.high].allSatisfy { $0.isFinite }
                #expect(finite, "\(x) \(y) \(equal)")
            }
        }
    }

    @Test func aChartOfLevelsReadsAnyValueAsALevel() {
        let stretches = AxisRules.stretches([0, 1, 1e19, nil, -1e19, 2.4])
        #expect(stretches.map(\.level) == [0, 1, Int.max, Int.min, 2])
    }

    // MARK: - Words

    @Test func aNumberPastAWholeNumberIsStillWritten() {
        let india = Locale(identifier: "en_IN")
        #expect(NumberWords.amount(100_000, locale: india) == "1,00,000")
        #expect(NumberWords.amount(12.5, locale: india) == "12.5")
        #expect(!NumberWords.amount(1e19, locale: india).isEmpty)
        #expect(!NumberWords.amount(9.3e18, locale: india).isEmpty)
        #expect(!NumberWords.amount(-1e308, locale: india).isEmpty)
        #expect(NumberWords.amount(.nan, locale: india) == "–")
        let rupees = ValueStyle(prefix: "₹", indian: true)
        #expect(ValueWords.text(4_000_000, rupees) == "₹40,00,000")
        #expect(ValueWords.text(1e19, rupees) == "₹1,00,00,00,00,00,00,00,00,000")
        #expect(ValueWords.text(-1e19, ValueStyle()) == "−10,000,000,000,000,000,000")
        #expect(!ValueWords.text(1e300, ValueStyle()).isEmpty)
        #expect(ValueWords.text(.infinity, ValueStyle(suffix: "%")) == "–%")
    }

    @Test func aRingAnyNumberOfTimesPastItsGoalIsOnlyPastIt() {
        #expect(RingRules.sweep(value: 750, max: 500) == RingRules.Sweep(laps: 1, part: 0.5))
        #expect(RingRules.sweep(value: 500, max: 500) == RingRules.Sweep(laps: 0, part: 1))
        #expect(RingRules.sweep(value: 1e19, max: 1)?.past == true)
        #expect(RingRules.sweep(value: 1, max: 1e-300)?.past == true)
        #expect(RingRules.sweep(value: 1e308, max: 1e-308)?.past == true)
        #expect(RingRules.sweep(value: .nan, max: 10) == nil)
        for sweep in [RingRules.sweep(value: 1e19, max: 1), RingRules.sweep(value: 1e308, max: 1e-308)] {
            #expect((0...1).contains(sweep?.part ?? -1))
        }
        #expect(ShareRules.percent(1e17) >= 1)
        #expect(ShareRules.percent(.nan) == 0)
        #expect(ClockRules.timeLabel(.infinity, uses24: true, locale: Locale(identifier: "en_IN")) == "00:00")
        #expect(ZoneRules.position(1e308, low: -1e308, high: 1e308) == 0 || ZoneRules.position(1e308, low: -1e308, high: 1e308) == 1)
    }

    // MARK: - Controls

    @Test func aSlidersEndsAndStepAreNumbersItCanBeGiven() {
        #expect(SliderRules.scale(min: "0", max: "10", step: "2") == SliderRules.Scale(range: 0...10, step: 2))
        #expect(SliderRules.scale(min: "0", max: "100", step: nil) == SliderRules.Scale(range: 0...100, step: 1))
        for (min, max, step) in [
            ("NaN", "100", "1"), ("0", "nan", "1"), ("inf", "infinity", "1"), ("-inf", "inf", nil), ("10", "0", "1"),
            ("5", "5", "1"), ("0", "100", "0"), ("0", "100", "-3"), ("0", "100", "NaN"), ("0", "100", "1000"),
            ("0", "1e19", "1"), ("", "", ""), ("0", "1", "1e-300"),
        ] as [(String, String, String?)] {
            let scale = SliderRules.scale(min: min, max: max, step: step)
            let width = scale.range.upperBound - scale.range.lowerBound
            #expect(scale.range.lowerBound < scale.range.upperBound, "\(min) \(max)")
            #expect(scale.range.lowerBound.isFinite && scale.range.upperBound.isFinite, "\(min) \(max)")
            #expect(scale.step > 0 && scale.step <= width, "\(String(describing: step))")
            #expect(width / scale.step <= SliderRules.mostStops * 1.000001, "\(min) \(max) \(String(describing: step))")
            // The value shown is always on the scale.
            for value in ["NaN", "inf", "-1e30", "", "3"] {
                #expect(scale.range.contains(SliderRules.value(value, in: scale)))
            }
        }
        #expect(FieldControl.number(1e19) == "1000000000000000")
        #expect(FieldControl.number(7) == "7")
        #expect(FieldControl.number(7.5) == "7.5")
    }

    @Test func roomAndSidesAPageAsksForAreHeldToWhatAScreenHas() {
        #expect(CompositionRules.points(steps: 3) == 12)
        #expect(CompositionRules.points(steps: -5) == 0)
        #expect(CompositionRules.points(steps: Int.max) == Double(CompositionRules.mostSteps) * 4)
        #expect(ImagePart.side(120) == 120)
        #expect(ImagePart.side(0) == nil)
        #expect(ImagePart.side(-40) == nil)
        #expect(ImagePart.side(Int.max) == 4096)
        #expect(ProgressPart.fraction(500) == 0.5)
        #expect(ProgressPart.fraction(-200) == 0)
        #expect(ProgressPart.fraction(Int.max) == 1)
    }

    // MARK: - Days and icons

    @Test func aCalendarOfDaysDrawsTwoYearsAtMost() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(identifier: "UTC"))
        let first = try #require(PagerRules.date("2026-01-01"))
        let year = try #require(PagerRules.date("2026-12-31"))
        #expect(HeatRules.weeks(first: first, last: year, calendar: calendar) == 53)
        let far = try #require(PagerRules.date("9999-12-31"))
        #expect(HeatRules.weeks(first: first, last: far, calendar: calendar) == HeatRules.mostWeeks)
        #expect(HeatRules.weeks(first: far, last: first, calendar: calendar) == 1)
        // A day no calendar has is no day.
        #expect(PagerRules.date("999999999-01-01") == nil)
        #expect(PagerRules.date("0-00-00") == nil)
        #expect(PagerRules.date("2026-13-40") == nil)
        #expect(PagerRules.date("2026-11-16") != nil)
    }

    @Test func anIconsPathWithANumberTooLargeToHoldStillDraws() {
        // An arc whose radius no number holds: the path stops there, with nothing drawn.
        #expect(SVGPath.path("M0 0 A1e400 1 0 0 1 5 5 L9 9").boundingRect.width == 0)
        // Radii and ends near the largest numbers there are: no angle can be worked out of them.
        let huge = SVGPath.path("M0 0 A1e200 1e200 0 0 1 1e200 1e200 L5 5")
        #expect(!huge.isEmpty)
        let far = SVGPath.path("M1e300 1e300 A3 3 0 1 0 -1e300 -1e300")
        #expect(!far.isEmpty)
        // An ordinary arc is still four curves at most.
        let circle = SVGPath.path("M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0")
        #expect(circle.boundingRect.width > 17 && circle.boundingRect.width < 19)
        #expect(IconShapeView.box("0 0 inf 24") == CGRect(x: 0, y: 0, width: 24, height: 24))
        #expect(IconShapeView.box("0 0 nan 24") == CGRect(x: 0, y: 0, width: 24, height: 24))
        #expect(IconShapeView.box("0 0 20 20") == CGRect(x: 0, y: 0, width: 20, height: 20))
    }
}
