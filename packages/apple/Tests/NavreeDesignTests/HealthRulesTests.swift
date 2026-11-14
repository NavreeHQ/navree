import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// The rules the health charts are measured, placed and coloured by, each
// with the case that proves it.

// MARK: Colours by name

@Test func eachHueIsTheSystemsOwnColour() {
    #expect(HueRules.system(Navree.Hue.violet) == .purple)
    #expect(HueRules.system(Navree.Hue.red) == .red)
    #expect(HueRules.system(Navree.Hue.green) == .green)
    #expect(HueRules.system(Navree.Hue.orange) == .orange)
    #expect(HueRules.system(Navree.Hue.blue) == .blue)
    #expect(HueRules.system(Navree.Hue.cyan) == .cyan)
    #expect(HueRules.system(Navree.Hue.rose) == .pink)
    #expect(HueRules.system(Navree.Hue.slate) == .gray)
    #expect(HueRules.system(Navree.Hue.accent) == .accent)
    #expect(HueRules.system(Navree.Hue.danger) == .red)
    #expect(HueRules.system(Navree.Hue.neutral) == .secondary)
}

@Test func amberStandsApartFromOrangeAndTheStatusHuesMatchTheirTones() {
    #expect(HueRules.system(Navree.Hue.amber) == .yellow)
    #expect(HueRules.system(Navree.Hue.amber) != HueRules.system(Navree.Hue.orange))
    #expect(HueRules.system(Navree.Hue.success) == HueRules.system(Navree.Tone.success))
    #expect(HueRules.system(Navree.Hue.warning) == HueRules.system(Navree.Tone.warning))
    #expect(HueRules.system(Navree.Hue.info) == HueRules.system(Navree.Tone.info))
    #expect(HueRules.system(nil as Navree.Tone?) == .secondary)
}

@Test func theFifteenHuesAreAllMappedAndOnlyThePlainOnesShareAColourWithATone() {
    let mapped = Set(Navree.Hue.allCases.map { HueRules.system($0) })
    #expect(mapped.count == 11)
}

@Test func theThemesOwnColourIsUsedOnlyForAHueThisClientDoesNotKnow() throws {
    // A hue newer than this client, as the protocol says it is read: neutral,
    // with the theme's own colours beside it.
    let unknown = Navree.Colour(dark: "#5eead4", hue: .neutral, light: "#0f766e")
    let light = try #require(HueRules.fallback(unknown, dark: false))
    #expect(abs(light.red - 15.0 / 255) < 0.001 && abs(light.green - 118.0 / 255) < 0.001)
    #expect(HueRules.fallback(unknown, dark: true)?.green ?? 0 > 0.9)
    #expect(HueRules.fallback(Navree.Colour(hue: .violet, light: "#7c3aed"), dark: false) == nil)
    #expect(HueRules.fallback(Navree.Colour(hue: .neutral), dark: false) == nil)
}

@Test func paleHuesAreDarkenedOnlyWhereTheyColourWordsOnALightGround() {
    #expect(HueRules.textShade(.yellow, dark: false) > HueRules.textShade(.cyan, dark: false))
    #expect(HueRules.textShade(.cyan, dark: false) > 0)
    #expect(HueRules.textShade(.purple, dark: false) == 0)
    #expect(SystemHue.allCases.allSatisfy { HueRules.textShade($0, dark: true) == 0 })
}

// MARK: Rings

@Test func aRingsSizeFollowsItsProminenceAndOnlyTheSmallOneKeepsItsSizeWithLargeText() {
    #expect(RingRules.diameter(.high) > RingRules.diameter(.medium))
    #expect(RingRules.diameter(.medium) > RingRules.diameter(.low))
    #expect(RingRules.diameter(.high) >= 176)
    #expect(RingRules.growth(.low) == 1)
    #expect(RingRules.growth(.high) > 1 && RingRules.growth(.high) <= 1.5)
    #expect(RingRules.stroke(.low, diameter: 34) / 34 > RingRules.stroke(.high, diameter: 188) / 188)
}

@Test func aRingGoesRoundAgainPastItsGoalAndIsEmptyWithNoValue() {
    #expect(RingRules.sweep(value: nil, max: 100) == nil)
    #expect(RingRules.sweep(value: 79, max: 100) == RingRules.Sweep(laps: 0, part: 0.79))
    #expect(RingRules.sweep(value: 100, max: 100) == RingRules.Sweep(laps: 0, part: 1))
    let past = RingRules.sweep(value: 6.3, max: 5)
    #expect(past?.laps == 1)
    #expect(abs((past?.part ?? 0) - 0.26) < 0.0001)
    #expect(past?.past == true)
    #expect(RingRules.sweep(value: 200, max: 100) == RingRules.Sweep(laps: 1, part: 1))
    #expect(RingRules.sweep(value: -5, max: 100) == RingRules.Sweep(laps: 0, part: 0))
    #expect(RingRules.sweep(value: 3, max: 0) == RingRules.Sweep(laps: 0, part: 1))
}

@Test func aSmallRingHasNoWordsAndAnEmptyRingSaysADash() {
    #expect(!RingRules.showsCentre(.low, centre: .value))
    #expect(!RingRules.showsCentre(.high, centre: .nothing))
    #expect(RingRules.showsCentre(.medium, centre: .value))
    #expect(!RingRules.saysWords(.low))
    #expect(RingRules.saysWords(.high))
    #expect(RingRules.centre(Navree.Ring(max: 100)) == "–")
    #expect(RingRules.centre(Navree.Ring(max: 100, shown: "79", value: 79)) == "79")
    #expect(RingRules.centre(Navree.Ring(max: 100, value: 66), locale: Locale(identifier: "en_IN")) == "66")
}

@Test func concentricRingsNestWithoutTouchingAndAtMostSixAreDrawn() {
    #expect(RingsRules.most == 6)
    let diameter = 150.0
    let stroke = RingsRules.stroke(diameter: diameter, count: 3)
    let outer = RingsRules.ringDiameter(diameter: diameter, index: 0, stroke: stroke)
    let second = RingsRules.ringDiameter(diameter: diameter, index: 1, stroke: stroke)
    #expect(abs(outer - (diameter - stroke)) < 0.0001)
    // The gap between the outer edge of the second ring and the inner edge of the first.
    #expect((outer - stroke) - (second + stroke) > 0)
    let innermost = RingsRules.ringDiameter(diameter: diameter, index: 2, stroke: stroke) - stroke
    #expect(innermost > diameter * 0.4)
    #expect(RingsRules.stroke(diameter: 150, count: 9) == RingsRules.stroke(diameter: 150, count: 6))
}

@Test func aRingsKeySaysEachValueOverItsGoalAsTheActivityRingsDo() {
    let india = Locale(identifier: "en_IN")
    #expect(RingsRules.value(Navree.RingValue(max: 500, name: "Move", shown: "441", unit: "kcal", value: 441), locale: india) == "441/500 kcal")
    #expect(RingsRules.value(Navree.RingValue(max: 10000, name: "Steps", shown: "8,324", value: 8324), locale: india) == "8,324/10,000")
    #expect(RingsRules.value(Navree.RingValue(max: 100000, name: "Steps", value: 100000), locale: india) == "1,00,000/1,00,000")
    #expect(RingsRules.value(Navree.RingValue(max: 5, name: "Distance", value: nil)) == "–")
    #expect(RingsRules.hasKey(Navree.Rings(centre: .values, legend: false, prominence: .medium)))
    #expect(!RingsRules.hasKey(Navree.Rings(legend: true, prominence: .low)))
}

// MARK: Zones, gauges and ranges

@Test func aValueIsInTheLastZoneThatHoldsItAndSitsAtTheNearerEndOutside() {
    let zones = [
        Navree.Zone(from: 0, label: "Under", to: 0.8),
        Navree.Zone(from: 0.8, label: "Optimal", to: 1.3),
        Navree.Zone(from: 1.3, label: "Over", to: 2),
    ]
    #expect(ZoneRules.zone(of: 0.93, in: zones)?.label == "Optimal")
    #expect(ZoneRules.zone(of: 0.8, in: zones)?.label == "Optimal")
    #expect(ZoneRules.index(of: 1.9, in: zones) == 2)
    #expect(ZoneRules.zone(of: nil, in: zones) == nil)
    #expect(ZoneRules.zone(of: 3, in: zones) == nil)
    #expect(ZoneRules.position(5.4, low: 2, high: 7) == 0.68)
    #expect(ZoneRules.position(-1, low: 0, high: 10) == 0)
    #expect(ZoneRules.position(12, low: 0, high: 10) == 1)
    #expect(ZoneRules.position(5, low: 3, high: 3) == 0)
}

@Test func aGaugesArcIsOpenAtTheFoot() {
    #expect(GaugeRules.sweep == 240)
    // From the lower left (150°) through the top (270°) to the lower right (30° + 360°).
    #expect(GaugeRules.angle(0) == 150)
    #expect(GaugeRules.angle(0.5) == 270)
    #expect(GaugeRules.angle(1) == 390)
    #expect(GaugeRules.diameter(aside: true) < GaugeRules.diameter(aside: false))
}

@Test func aGaugeWritesItsNumberLargeAndItsUnitUnderIt() {
    #expect(GaugeRules.split("44.2 mL/kg/min") == ("44.2", "mL/kg/min"))
    #expect(GaugeRules.split("0.93") == ("0.93", nil))
    #expect(GaugeRules.split("1,420 steps") == ("1,420", "steps"))
    #expect(GaugeRules.split("Very high") == ("Very high", nil))
}

@Test func aPercentFollowsItsNumberAndOtherUnitsStandApart() {
    #expect(NumberWords.withUnit("3.1", unit: "%") == "3.1%")
    #expect(NumberWords.withUnit("5.4", unit: "km/h") == "5.4 km/h")
    #expect(NumberWords.withUnit("7", unit: nil) == "7")
    #expect(NumberWords.amount(1_00_000, locale: Locale(identifier: "en_IN")) == "1,00,000")
    #expect(NumberWords.amount(6.25, locale: Locale(identifier: "en_IN")) == "6.3" || NumberWords.amount(6.25, locale: Locale(identifier: "en_IN")) == "6.2")
    #expect(NumberWords.amount(40.0) == "40")
}

// MARK: The day clock

@Test func theDayClockHasMidnightAtTheTopAndGoesClockwise() {
    #expect(ClockRules.angle(hours: 0) == -90)
    #expect(ClockRules.angle(hours: 6) == 0)
    #expect(ClockRules.angle(hours: 12) == 90)
    #expect(ClockRules.angle(hours: 18) == 180)
    #expect(ClockRules.quarters == [0, 6, 12, 18])
}

@Test func anHoursBarReachesByItsLevelElseItsValueAndAnHourWithNoReadingHasNone() {
    #expect(ClockRules.reach(Navree.Hour(level: 0.4, value: 900), most: 1000) == 0.4)
    #expect(ClockRules.reach(Navree.Hour(value: 710), most: 1420) == 0.5)
    #expect(ClockRules.reach(Navree.Hour(value: 0), most: 1420) == 0)
    #expect(ClockRules.reach(Navree.Hour(), most: 1420) == nil)
    #expect(ClockRules.reach(Navree.Hour(level: 3), most: nil) == 1)
    let clock = Navree.DayClock(hours: [Navree.Hour(value: 3), Navree.Hour(value: 9), Navree.Hour()])
    #expect(ClockRules.most(clock) == 9)
    var capped = clock
    capped.max = 20
    #expect(ClockRules.most(capped) == 20)
}

@Test func aMomentsTimeIsWrittenInThePersonsClock() {
    let india = Locale(identifier: "en_IN")
    let twelve = ClockRules.timeLabel(7.116666666666666, uses24: false, locale: india)
    #expect(twelve.hasPrefix("7:07"))
    #expect(twelve.lowercased().contains("am"))
    #expect(ClockRules.timeLabel(18.25, uses24: true, locale: india) == "18:15")
    #expect(ClockRules.timeLabel(24, uses24: true, locale: india) == "00:00")
    #expect(ClockRules.timeLabel(-0.5, uses24: true, locale: india) == "23:30")
    #expect(ClockRules.quarterLabel(18, uses24: true, locale: india) == "18")
    #expect(ClockRules.quarterLabel(0, uses24: false, locale: india).hasPrefix("12"))
    #expect(ClockRules.uses24(Locale(identifier: "de_DE")))
    #expect(ClockRules.momentEntry("Woke up", at: 7.1166, uses24: true, locale: india) == "Woke up, 07:07")
    #expect(ClockRules.momentEntry("Woke up at 7:07 am", at: 7.1166, uses24: true, locale: india) == "Woke up at 7:07 am")
    #expect(ClockRules.holdsTime("Woke up, 6:30 am"))
    #expect(!ClockRules.holdsTime("Most active hour, 1,420 steps"))
}

@Test func momentsTooCloseOnTheDialStandApartAndTheRestStay() {
    let placed = ClockRules.spread([18, 7.1, 18.25], gap: 1)
    #expect(placed[1] == 7.1)
    #expect(abs((placed[2] - placed[0]) - 1) < 0.0001)
    #expect(abs((placed[0] + placed[2]) / 2 - 18.125) < 0.0001)
    #expect(ClockRules.spread([1, 5, 9], gap: 1) == [1, 5, 9])
}

@Test func theDialHasItsLabelsOutsideThenTheMomentsThenTheBarsAndRoomInTheMiddle() {
    let dial = ClockRules.dial(half: 150, label: 15, mark: 22)
    #expect(dial.labels > dial.moments)
    #expect(dial.moments > dial.outer)
    #expect(dial.outer > dial.inner)
    #expect(dial.inner >= 40)
    #expect(ClockRules.barWidth(inner: dial.inner) < 2 * Double.pi * dial.inner / 24)
}

// MARK: Keys, shares, calendars, matrices

@Test func aKeySaysThePagesOwnWordsBesideTheName() {
    #expect(KeyRules.entry("Deep", detail: "15.6% of the night") == "Deep · 15.6% of the night")
    #expect(KeyRules.entry("Deep", detail: nil) == "Deep")
    #expect(KeyRules.entry("Deep", detail: "") == "Deep")
}

@Test func sharesAreOfTheWholeAndAPartWithSomeIsNeverNoPercent() {
    #expect(ShareRules.shares([1, 3, nil]) == [0.25, 0.75, 0])
    #expect(ShareRules.shares([0, 0]) == [0, 0])
    #expect(ShareRules.percent(0.004) == 1)
    #expect(ShareRules.percent(0) == 0)
    #expect(ShareRules.percent(0.156) == 16)
    #expect(ShareRules.saysInside(0.5, width: 300, textWidth: 30))
    #expect(!ShareRules.saysInside(0.03, width: 300, textWidth: 30))
    let cells = [Navree.StripCell(category: 0, label: "a"), Navree.StripCell(category: 2, label: "b"), Navree.StripCell(label: "c"), Navree.StripCell(category: 2, label: "d")]
    let shares = ShareRules.categoryShares(cells, keySize: 3)
    #expect(abs(shares[0] - 1.0 / 3) < 0.0001)
    #expect(shares[1] == 0)
    #expect(abs(shares[2] - 2.0 / 3) < 0.0001)
}

@Test func aCalendarDayIsShadedByThePagesStepsAndADayWithNoValueHasNoShade() {
    #expect(HeatRules.shade(nil, steps: [50, 100, 200], highest: 300) == nil)
    #expect(HeatRules.shade(0, steps: [50, 100, 200], highest: 300) == 0)
    #expect(HeatRules.shade(99, steps: [50, 100, 200], highest: 300) == 1)
    #expect(HeatRules.shade(200, steps: [50, 100, 200], highest: 300) == 3)
    // Without steps: four equal parts of the highest.
    #expect(HeatRules.shade(60, steps: [], highest: 100) == 2)
    #expect(HeatRules.opacities.count == 4)
}

@Test func aCalendarHasAWeekAColumnFromThePersonsFirstWeekday() throws {
    var monday = Calendar(identifier: .gregorian)
    monday.timeZone = TimeZone(identifier: "UTC")!
    monday.firstWeekday = 2
    let first = try #require(PagerRules.date("2026-05-16"))  // a Saturday
    let start = HeatRules.start(first, calendar: monday)
    #expect(PagerRules.text(start) == "2026-05-11")
    let place = HeatRules.cell(try #require(PagerRules.date("2026-05-18")), start: start, calendar: monday)
    #expect(place.week == 1 && place.day == 0)
    #expect(HeatRules.weeks(first: first, last: try #require(PagerRules.date("2026-11-12")), calendar: monday) == 27)
    var sunday = monday
    sunday.firstWeekday = 1
    #expect(PagerRules.text(HeatRules.start(first, calendar: sunday)) == "2026-05-10")
    let months = HeatRules.months(first: first, weeks: 27, calendar: monday)
    #expect(months.map(\.month) == [5, 6, 7, 8, 9, 10, 11])
}

@Test func aMatrixIsReadEitherWayRoundAndChanceIsNoStrength() {
    let relations = [
        Navree.Relation(a: 0, b: 3, value: 0.71),
        Navree.Relation(a: 1, b: 3, significant: false, value: -0.34),
        Navree.Relation(a: 0, b: 2),
    ]
    #expect(MatrixRules.relation(relations, 3, 0)?.value == 0.71)
    #expect(MatrixRules.strength(MatrixRules.relation(relations, 3, 0), peak: 1) == 0.71)
    #expect(MatrixRules.strength(MatrixRules.relation(relations, 1, 3), peak: 1) == 0)
    #expect(MatrixRules.strength(MatrixRules.relation(relations, 0, 2), peak: 1) == 0)
    #expect(MatrixRules.strength(Navree.Relation(a: 0, b: 1, value: -0.9), peak: 0.5) == 1)
    #expect(MatrixRules.drawn(row: 2, column: 1, lower: true))
    #expect(!MatrixRules.drawn(row: 1, column: 2, lower: true))
    #expect(!MatrixRules.drawn(row: 1, column: 1, lower: false))
}

// MARK: Charts

@Test func aChartsValueAxisStandsOnZeroForBarsAndIsRoundedOut() {
    let bars = Navree.Chart(kind: .bar, labels: ["a", "b"], references: [Navree.Reference(kind: .level, value: 10000)], series: [Navree.Series(label: "Steps", values: [7054, 23361])])
    #expect(AxisRules.values(bars) == AxisRules.Bounds(0, 25000))
    let heart = Navree.Chart(kind: .area, labels: ["a", "b"], series: [Navree.Series(label: "HR", values: [52, 89])])
    let bounds = AxisRules.values(heart)
    #expect(bounds.low < 52 && bounds.low > 0)
    #expect(bounds.high >= 89)
    let fixed = Navree.Chart(kind: .line, series: [Navree.Series(label: "x", values: [3])], y: Navree.Measure(max: 10, min: 0))
    #expect(AxisRules.values(fixed) == AxisRules.Bounds(0, 10))
    let stages = Navree.Chart(kind: .line, levels: ["Deep", "Core", "REM", "Awake"], series: [Navree.Series(label: "Stage", values: [1, 0, 3])])
    #expect(AxisRules.values(stages) == AxisRules.Bounds(0, 3))
    #expect(AxisRules.nice(0, 0) == AxisRules.Bounds(0, 1))
}

@Test func aLongAxisIsThinnedToAHandfulOfThePagesWordsAStepAPersonReads() {
    #expect(AxisRules.ticks(count: 7, most: 8) == Array(0..<7))
    #expect(AxisRules.ticks(count: 96, most: 5) == [0, 20, 40, 60, 80])
    #expect(AxisRules.ticks(count: 30, most: 5) == [0, 6, 12, 18, 24])
    #expect(AxisRules.ticks(count: 87, most: 4) == [0, 24, 48, 72])
    #expect(AxisRules.ticks(count: 14, most: 3) == [0, 5, 10])
    #expect(AxisRules.ticks(count: 0, most: 5) == [])
    // The first label starts at its mark: a gap holds a label and a half.
    #expect(AxisRules.most(width: 300, label: 48) == 3)
    #expect(AxisRules.most(width: 260, label: 39) == 3)
    #expect(AxisRules.most(width: 10, label: 48) == 1)
    // A plot not yet laid out names one day, never all of them.
    #expect(AxisRules.most(width: .infinity, label: 35) == 1)
    #expect(AxisRules.most(width: 0, label: 35) == 1)
}

@Test func theDaysAlongAChartsFootNeverRunIntoEachOther() {
    // The store's day on an iPad: 30 days of "16 Oct" (35 points in the
    // axis's type) under a plot of 297 points in a carousel's card.
    let most = AxisRules.most(width: 297, label: 35)
    #expect(AxisRules.ticks(count: 30, most: most) == [0, 10, 20])
    // At every plot width and number of days, two marks named stand a label
    // and a half and a space apart: the first label, set from its mark, never
    // runs into the next, which stands over its middle.
    for count in [7, 14, 30, 31, 90, 96] {
        for width in stride(from: 120.0, through: 1200, by: 37) {
            for label in [20.0, 35, 48] {
                let marks = AxisRules.ticks(count: count, most: AxisRules.most(width: width, label: label))
                guard marks.count > 1 else { continue }
                let gap = Double(marks[1] - marks[0]) * width / Double(count)
                #expect(gap >= label * 1.5 + 12, "count \(count), width \(width), label \(label)")
            }
        }
    }
}

@Test func aGapIsAGapTheLineBreaksThere() {
    #expect(AxisRules.runs([1, 2, nil, nil, 3, nil, 4]) == [0, 0, 1, 1, 1, 2, 2])
    #expect(AxisRules.runs([nil, 1]) == [1, 1])
    let stretches = AxisRules.stretches([1, 1, 0, 0, 0, 3, nil, 3])
    #expect(stretches.map(\.level) == [1, 0, 3, 3])
    #expect(stretches.map(\.from) == [0, 2, 5, 7])
    #expect(stretches.map(\.to) == [1, 4, 5, 7])
}

@Test func aScatterWithEqualAxesHasOneRangeForBoth() {
    let plot = Navree.Scatter(equalAxes: true, series: [Navree.PointSeries(name: "a", points: [Navree.Point(x: 680, y: 830), Navree.Point(x: 845, y: 700)])])
    let axes = AxisRules.scatter(plot)
    #expect(axes.x == axes.y)
    #expect(axes.x.low <= 680 && axes.x.high >= 845)
    let free = AxisRules.scatter(Navree.Scatter(series: [Navree.PointSeries(name: "a", points: [Navree.Point(x: 1, y: 100)])]))
    #expect(free.x != free.y)
}

@Test func aChartDoesNotRepeatTheWordsTheScreenSetOverIt() {
    let siblings: [Navree.Node] = [
        .text(Navree.Text(style: .headline, value: "Steps, last 30 days")),
        .text(Navree.Text(value: "Goal met on 10 of 30 days")),
        .chart(Navree.Chart(title: "Steps, last 30 days")),
    ]
    let above = HeadingRules.wordsAbove(siblings, index: 2)
    #expect(above == ["Steps, last 30 days", "Goal met on 10 of 30 days"])
    #expect(!HeadingRules.showsTitle("Steps, last 30 days", wordsAbove: above))
    #expect(HeadingRules.showsTitle("Load by week", wordsAbove: ["Training"]))
    #expect(!HeadingRules.showsTitle(nil, wordsAbove: []))
    #expect(HeadingRules.wordsAbove(siblings, index: 0) == [])
}

// MARK: Progress, trends, the pager

@Test func aBarsColourIsItsMeasuresThenItsMeaningThenTheAccent() {
    let both = Navree.Progress(colour: Navree.Colour(hue: .blue), tone: .warning, value: 500)
    #expect(ProgressRules.fill(both) == .measure(Navree.Colour(hue: .blue)))
    #expect(ProgressRules.fill(Navree.Progress(tone: .success, value: 750)) == .tone(.success))
    #expect(ProgressRules.fill(Navree.Progress(value: 1)) == .accent)
    #expect(ProgressRules.fraction(Navree.Progress(value: 580)) == 0.58)
    #expect(ProgressRules.fraction(Navree.Progress(value: 1400)) == 1)
    #expect(ProgressRules.fraction(Navree.Progress()) == nil)
}

@Test func aTrendTakesTheSystemsArrowAndTheColourOfItsNews() {
    #expect(TrendRules.symbol(.up) == "arrow.up.right")
    #expect(TrendRules.symbol(.down) == "arrow.down.right")
    #expect(TrendRules.symbol(.flat) == "arrow.right")
    #expect(TrendRules.hue(Navree.Trend(direction: .up, tone: .danger)) == .red)
    #expect(TrendRules.hue(Navree.Trend(direction: .down, tone: .success)) == .green)
    #expect(TrendRules.hue(Navree.Trend(direction: .up)) == .secondary)
}

@Test func onlyTheDaysThePagerOffersMayBeChosenAndTheDayIsSentAsThePageWritesIt() throws {
    #expect(PagerRules.choosable("2026-11-12", first: "2026-04-17", last: "2026-11-12"))
    #expect(!PagerRules.choosable("2026-11-13", first: "2026-04-17", last: "2026-11-12"))
    #expect(!PagerRules.choosable("2026-04-16", first: "2026-04-17", last: nil))
    #expect(PagerRules.choosable("1999-01-01", first: nil, last: nil))
    #expect(!PagerRules.choosable("not a day", first: nil, last: nil))
    #expect(PagerRules.text(try #require(PagerRules.date("2026-11-03"))) == "2026-11-03")
    let pager = Navree.DayPager(date: "2026-11-12", day: "Yesterday", first: "2026-04-17", last: "2026-11-12")
    let bounds = try #require(PagerRules.bounds(pager))
    #expect(PagerRules.text(bounds.lowerBound) == "2026-04-17")
    #expect(PagerRules.bounds(Navree.DayPager(date: "x", day: "x", first: "2026-11-12", last: "2026-04-17")) == nil)
}

@Test func aPagerThatLeadsAScreensBodyStandsInTheBarAndOnlyThere() {
    let pager = Navree.DayPager(date: "2026-11-13", day: "Today")
    let read = Navree.Screen(body: .stack(Navree.Stack(children: [.dayPager(pager), .text(Navree.Text(value: "Hello"))])), title: "Today")
    #expect(PagerRules.inBar(read) == pager)
    let lifted = PagerRules.lifted(read)
    #expect(lifted.pager == pager)
    guard case .stack(let stack) = lifted.body else { Issue.record("the body is no longer a stack"); return }
    #expect(stack.children.count == 1)
    let written = Navree.Screen(body: .stack(Navree.Stack(children: [.text(Navree.Text(value: "Hello")), .dayPager(pager)])), pager: pager, title: "Home")
    #expect(PagerRules.lifted(written) == written)
    let later = Navree.Screen(body: .stack(Navree.Stack(children: [.text(Navree.Text(value: "Hello")), .dayPager(pager)])), title: "Sleep")
    #expect(PagerRules.inBar(later) == nil)
    #expect(PagerRules.lifted(later) == later)
}

@Test func aDayControlNeverTakesTheTitlesPlaceItStandsUnderALargeTitleOrHeadsTheContent() {
    let pager = Navree.DayPager(date: "2026-11-12", day: "Yesterday")
    // The screen's name stays, large at a destination's root, inline when pushed.
    #expect(PagerRules.titleMode(.root, pager: pager) == .large)
    #expect(PagerRules.titleMode(.pushed, pager: pager) == .inline)
    #expect(PagerRules.titleMode(.root, pager: nil) == NavigationRules.titleMode(.root))
    // Under a large title, in the bar; under an inline one, at the head of the content.
    #expect(PagerRules.place(.root) == .underTitle)
    #expect(PagerRules.place(.pushed) == .headOfContent)
    #expect(PagerRules.place(.sheet) == .headOfContent)
    let text = Navree.Node.text(Navree.Text(value: "Hello"))
    let screen = Navree.Screen(body: .stack(Navree.Stack(children: [text])), pager: pager, title: "Sleep")
    #expect(PagerRules.placed(screen, .underTitle) == screen)
    let headed = PagerRules.placed(screen, .headOfContent)
    #expect(headed.pager == nil)
    #expect(headed.title == "Sleep")
    #expect(headed.body == .stack(Navree.Stack(children: [.dayPager(pager), text])))
    #expect(PagerRules.words(pager).hasPrefix("Yesterday, "))
}

// MARK: Symbols and sections

@Test func anIconTakesTheSystemSymbolThePageNamesForIt() {
    let icon = Navree.Icon(name: "heart-pulse", symbol: Navree.SystemSymbol(apple: "bolt.heart", appleFilled: "bolt.heart.fill", material: "monitor_heart"))
    #expect(Symbols.symbol(for: icon) == "bolt.heart")
    #expect(Symbols.symbol(for: Navree.Icon(name: "house")) == "house")
    #expect(Symbols.symbol(for: Navree.Icon(name: "route")) == nil)
}

@Test func aCentredStackInAListIsOneRowSoARingKeepsItsWordsUnderIt() {
    let ring = Navree.Node.ring(Navree.Ring(max: 100, value: 83))
    let centred = Navree.Node.stack(Navree.Stack(align: .center, children: [ring, .text(Navree.Text(value: "Excellent"))]))
    let sections = SectionRules.sections(.section(Navree.Section(children: [centred])))
    #expect(sections.first?.rows == [.part(centred)])
    let plain = SectionRules.sections(.section(Navree.Section(children: [.stack(Navree.Stack(children: [ring, .text(Navree.Text(value: "a"))]))])))
    #expect(plain.first?.rows.count == 2)
}
