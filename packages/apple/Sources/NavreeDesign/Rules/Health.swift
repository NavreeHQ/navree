import Foundation
import NavreeProtocol

// The measuring, placing and colouring rules of the health charts, as plain
// functions of the tree, the room and the person's settings, so each can be
// tested without drawing. Drawing them is in Draw/.

/// The system colours a part's hue is drawn in. Each is one of the system's
/// own named colours, which adapt by themselves to dark mode and to
/// increased contrast.
public enum SystemHue: String, Sendable, CaseIterable {
    /// The app's tint.
    case accent
    /// The secondary label colour: what has no colour of its own.
    case secondary
    case red, orange, yellow, green, mint, teal, cyan, blue, indigo, purple, pink, brown, gray
}

public enum HueRules {
    /// The system colour a hue is drawn in. The protocol's status tones take
    /// the colours the rest of the client gives them (warning is orange), so
    /// `amber`, which must stand apart from `orange`, is the system's yellow.
    public static func system(_ hue: Navree.Hue) -> SystemHue {
        switch hue {
        case .neutral: .secondary
        case .accent: .accent
        case .success, .green: .green
        case .warning, .orange: .orange
        case .danger, .red: .red
        case .info, .blue: .blue
        case .amber: .yellow
        case .rose: .pink
        case .violet: .purple
        case .cyan: .cyan
        case .slate: .gray
        }
    }

    /// The system colour of a tone.
    public static func system(_ tone: Navree.Tone?) -> SystemHue {
        switch tone {
        case .success: .green
        case .warning: .orange
        case .danger: .red
        case .info: .blue
        case .accent: .accent
        case .neutral, nil: .secondary
        }
    }

    /// The theme's own colour for a part whose hue this client read as
    /// `neutral` (a hue newer than this client is read so), when the server
    /// sent one: `#rrggbb` for the appearance. Every hue this client knows is
    /// drawn in the system's colour instead.
    public static func fallback(_ colour: Navree.Colour, dark: Bool) -> (red: Double, green: Double, blue: Double)? {
        guard colour.hue == .neutral, let hex = dark ? (colour.dark ?? colour.light) : (colour.light ?? colour.dark) else { return nil }
        return rgb(hex)
    }

    static func rgb(_ hex: String) -> (red: Double, green: Double, blue: Double)? {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        return (Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255, Double(value & 0xFF) / 255)
    }

    /// How much darker a hue is made where it colours words on a light ground,
    /// 0 to 1: the system's yellow and cyan are bright enough for a mark but
    /// too pale for text there. On a dark ground every hue reads as it is.
    public static func textShade(_ hue: SystemHue, dark: Bool) -> Double {
        guard !dark else { return 0 }
        switch hue {
        case .yellow: return 0.38
        case .cyan, .mint: return 0.22
        case .green, .teal: return 0.12
        default: return 0
        }
    }

    /// The hues the series of a chart take, in order, when the page names none.
    public static let series: [SystemHue] = [.accent, .orange, .green, .purple, .pink, .cyan, .yellow, .red]
}

/// A ring: one value against its goal, a second lap past it.
public enum RingRules {
    /// The ring's width in points before the person's text size: the hero
    /// ring, one of several, a small one beside words.
    public static func diameter(_ prominence: Navree.Prominence) -> Double {
        switch prominence {
        case .high: 188
        case .medium: 96
        case .low: 34
        }
    }

    /// The arc's width for a ring that wide. A small ring keeps a thicker arc
    /// for its size, so it still reads as a ring.
    public static func stroke(_ prominence: Navree.Prominence, diameter: Double) -> Double {
        switch prominence {
        case .high: diameter * 0.115
        case .medium: diameter * 0.12
        case .low: diameter * 0.16
        }
    }

    /// How much a ring grows with the person's text, at most: the hero and the
    /// medium rings hold words, a small ring stands in a row of text and keeps
    /// its size so the row does not break.
    public static func growth(_ prominence: Navree.Prominence) -> Double {
        switch prominence {
        case .high: 1.3
        case .medium: 1.5
        case .low: 1
        }
    }

    /// The size of the number in a ring's middle: 0.3 of the ring at the
    /// text size the ring was drawn for, and growing on with the person's
    /// text past the size the ring stops growing at (the number then fits
    /// itself into the ring), so it follows every text size.
    public static func centreSize(diameter: Double, textScale: Double, ringScale: Double) -> Double {
        diameter * 0.3 * max(textScale, 0.1) / max(ringScale, 0.1)
    }

    /// Whether words go in the middle: a small ring says its value to VoiceOver only.
    public static func showsCentre(_ prominence: Navree.Prominence, centre: Navree.Centre) -> Bool {
        prominence != .low && centre != .nothing
    }

    /// Whether the ring says its label, status and trend under it: a small ring
    /// has no room, and its summary says them.
    public static func saysWords(_ prominence: Navree.Prominence) -> Bool {
        prominence != .low
    }

    /// How far round a value goes: the whole laps past the goal and the part
    /// of the lap it is on. `nil` for no value (an empty ring and a dash).
    public static func sweep(value: Double?, max: Double) -> Sweep? {
        guard let value else { return nil }
        guard max > 0 else { return Sweep(laps: 0, part: value > 0 ? 1 : 0) }
        let share = Swift.max(value / max, 0)
        let laps = Int(share.rounded(.down))
        let part = share - Double(laps)
        // Exactly at the goal is one whole lap, not a second one starting.
        if laps >= 1, part == 0 { return Sweep(laps: laps - 1, part: 1) }
        return Sweep(laps: laps, part: part)
    }

    public struct Sweep: Equatable, Sendable {
        public var laps: Int
        public var part: Double

        public init(laps: Int, part: Double) {
            self.laps = laps
            self.part = part
        }

        /// Whether the value is past its goal: the arc goes round again.
        public var past: Bool { laps >= 1 }
    }

    /// What the middle of a ring says: its value without the unit, or a dash
    /// when it has none.
    public static func centre(_ ring: Navree.Ring, locale: Locale = .current) -> String {
        guard let value = ring.value else { return "–" }
        return ring.shown ?? NumberWords.amount(value, locale: locale)
    }
}

/// Concentric rings, the first outside, as the system's activity rings.
public enum RingsRules {
    /// At most six are drawn, as the protocol says.
    public static let most = 6

    /// The width of the whole, in points, before the person's text size.
    public static func diameter(_ prominence: Navree.Prominence, count: Int) -> Double {
        switch prominence {
        case .low: 30
        case .medium: 150 + 10 * Double(Swift.max(count - 3, 0))
        case .high: 188
        }
    }

    /// The width of each arc for a whole this wide: the rings and their gaps
    /// take a little under half the radius, so the middle keeps room. A low
    /// whole (a mark beside words) is all rings.
    public static func stroke(diameter: Double, count: Int, prominence: Navree.Prominence = .medium) -> Double {
        let n = Double(Swift.min(Swift.max(count, 1), most))
        let share = prominence == .low ? 0.82 : 0.5
        return (diameter * share / 2) / (n + (n - 1) * gapShare)
    }

    /// The gap between two rings, as a share of the arc's width.
    public static let gapShare = 0.12

    /// The diameter of ring `index` (0 the outside one), at the middle of its arc.
    public static func ringDiameter(diameter: Double, index: Int, stroke: Double) -> Double {
        diameter - stroke - Double(index) * 2 * stroke * (1 + gapShare)
    }

    /// The words a ring's key says of its value: the value over its goal with
    /// the unit, as the activity rings say them ("441/500 kcal").
    public static func value(_ ring: Navree.RingValue, locale: Locale = .current) -> String {
        guard let value = ring.value else { return "–" }
        let shown = ring.shown ?? NumberWords.amount(value, locale: locale)
        let goal = NumberWords.amount(ring.max, locale: locale)
        return ring.unit.map { "\(shown)/\(goal) \($0)" } ?? "\(shown)/\(goal)"
    }

    /// Whether the rings stand beside a key of their values.
    public static func hasKey(_ rings: Navree.Rings) -> Bool {
        rings.prominence != .low && (rings.legend || rings.centre == .values)
    }
}

/// Numbers in the person's way of writing them.
public enum NumberWords {
    /// A number with the decimals it needs and no more, grouped as the
    /// locale groups ("1,00,000" in India).
    public static func amount(_ value: Double, locale: Locale = .current) -> String {
        let rounded = (value * 10).rounded() / 10
        if rounded == rounded.rounded() {
            return Int(rounded).formatted(.number.locale(locale))
        }
        return rounded.formatted(.number.precision(.fractionLength(0...1)).locale(locale))
    }
}

/// A chart's own title beside the words a screen sets over it.
public enum HeadingRules {
    /// The words of the texts just above a part among its siblings (the two
    /// before it), which the part's own title may repeat.
    public static func wordsAbove(_ siblings: [Navree.Node], index: Int) -> [String] {
        siblings[max(index - 2, 0)..<min(index, siblings.count)].compactMap { node in
            if case .text(let text) = node { return text.value }
            return nil
        }
    }

    /// Whether a chart says its own title: not when the screen has just said
    /// the same words over it.
    public static func showsTitle(_ title: String?, wordsAbove: [String]) -> Bool {
        guard let title, !title.isEmpty else { return false }
        return !wordsAbove.contains { $0.caseInsensitiveCompare(title) == .orderedSame }
    }
}

extension NumberWords {
    /// An amount with its unit: a space between them, except before a
    /// percent sign, which follows its number as the locale writes it.
    public static func withUnit(_ amount: String, unit: String?) -> String {
        guard let unit, !unit.isEmpty else { return amount }
        return unit == "%" ? amount + unit : "\(amount) \(unit)"
    }
}

/// Zones along a scale: a gauge's, a range's, a chart's.
public enum ZoneRules {
    /// The zone a value is in: the last one that holds it, as the protocol says.
    public static func zone(of value: Double?, in zones: [Navree.Zone]) -> Navree.Zone? {
        guard let value else { return nil }
        return zones.last { value >= Swift.min($0.from, $0.to) && value <= Swift.max($0.from, $0.to) }
    }

    /// The place of the zone a value is in.
    public static func index(of value: Double?, in zones: [Navree.Zone]) -> Int? {
        guard let zone = zone(of: value, in: zones) else { return nil }
        return zones.lastIndex(of: zone)
    }

    /// Where a value sits between two ends, 0 to 1; outside, at the nearer end.
    public static func position(_ value: Double, low: Double, high: Double) -> Double {
        guard high > low else { return 0 }
        return Swift.min(Swift.max((value - low) / (high - low), 0), 1)
    }
}

/// A gauge: a value on an open arc.
public enum GaugeRules {
    /// The arc runs through this many degrees, open at the foot, as the
    /// system's circular gauge is.
    public static let sweep = 240.0

    /// The angle of the arc's start, in SwiftUI's degrees (0 at three o'clock,
    /// going clockwise): the lower left.
    public static let start = 150.0

    /// The angle a place along the scale stands at, 0 to 1.
    public static func angle(_ position: Double) -> Double {
        start + sweep * position
    }

    /// The share of its square an open arc needs in height: the foot of the
    /// square, below the arc's ends, holds the scale's end labels.
    public static let heightShare = 0.84

    /// A gauge's words as the number to write large and what follows it
    /// ("44.2 mL/kg/min" is 44.2 over mL/kg/min); words that do not start
    /// with a number are written whole.
    public static func split(_ shown: String) -> (number: String, unit: String?) {
        guard let space = shown.firstIndex(of: " "),
            shown[..<space].contains(where: \.isNumber),
            shown[..<space].allSatisfy({ $0.isNumber || ",.-−+".contains($0) })
        else { return (shown, nil) }
        let rest = shown[shown.index(after: space)...].trimmingCharacters(in: .whitespaces)
        return (String(shown[..<space]), rest.isEmpty ? nil : rest)
    }

    /// The base width of a gauge before the person's text size: one that leads
    /// its card, and one beside a figure's words.
    public static func diameter(aside: Bool) -> Double {
        aside ? 52 : 184
    }
}

/// The day clock: 24 hours round a dial, midnight at the top.
public enum ClockRules {
    /// The angle in SwiftUI's degrees (0 at three o'clock, clockwise) of a time
    /// of day in hours.
    public static func angle(hours: Double) -> Double {
        -90 + hours * 15
    }

    /// How far out an hour's mark reaches, 0 to 1: its level, else its value
    /// against the most, else nothing (an hour with no reading).
    public static func reach(_ hour: Navree.Hour, most: Double?) -> Double? {
        if let level = hour.level { return Swift.min(Swift.max(level, 0), 1) }
        guard let value = hour.value, let most else { return nil }
        return most <= 0 ? 0 : Swift.min(Swift.max(value / most, 0), 1)
    }

    /// The most an hour holds: the clock's own `max`, else the busiest hour.
    public static func most(_ clock: Navree.DayClock) -> Double? {
        clock.max ?? clock.hours.compactMap(\.value).max()
    }

    /// The hours named round the dial: the four quarters, midnight at the top.
    public static let quarters = [0, 6, 12, 18]

    /// An hour as the person's clock writes it round the dial: "12 AM" and
    /// "6 PM" on a 12 hour clock, "00" and "18" on a 24 hour one.
    public static func quarterLabel(_ hour: Int, uses24: Bool, locale: Locale) -> String {
        if uses24 { return String(format: "%02d", hour % 24) }
        let format = Date.FormatStyle(locale: locale, timeZone: utc).hour(.defaultDigits(amPM: .abbreviated))
        return date(hours: Double(hour)).formatted(format)
    }

    /// A time of day in hours past midnight as the person's clock writes it:
    /// "7:07 AM" on a 12 hour clock, "07:07" on a 24 hour one.
    public static func timeLabel(_ at: Double, uses24: Bool, locale: Locale) -> String {
        let minutes = Int((((at.truncatingRemainder(dividingBy: 24)) + 24).truncatingRemainder(dividingBy: 24) * 60).rounded()) % (24 * 60)
        if uses24 { return String(format: "%02d:%02d", minutes / 60, minutes % 60) }
        let format = Date.FormatStyle(locale: locale, timeZone: utc).hour(.defaultDigits(amPM: .abbreviated)).minute(.twoDigits)
        return date(hours: Double(minutes) / 60).formatted(format)
    }

    /// Whether the person's locale writes the time on a 24 hour clock.
    public static func uses24(_ locale: Locale) -> Bool {
        locale.hourCycle == .zeroToTwentyThree || locale.hourCycle == .oneToTwentyFour
    }

    private static let utc = TimeZone(identifier: "UTC")!

    private static func date(hours: Double) -> Date {
        Date(timeIntervalSince1970: (hours * 3600).rounded())
    }

    /// Where the day's moments stand on the dial: at their hour, but no two
    /// closer than `gap` hours, so marks that would sit on one another stand
    /// side by side. Neighbours that are too close move apart about their
    /// middle; the order stays, and moments far apart do not move.
    public static func spread(_ ats: [Double], gap: Double) -> [Double] {
        let order = ats.indices.sorted { ats[$0] < ats[$1] }
        var placed = order.map { ats[$0] }
        for _ in 0..<40 {
            for i in placed.indices.dropFirst() where placed[i] - placed[i - 1] < gap {
                let middle = (placed[i] + placed[i - 1]) / 2
                placed[i - 1] = middle - gap / 2
                placed[i] = middle + gap / 2
            }
        }
        var out = Array(repeating: 0.0, count: ats.count)
        for (rank, index) in order.enumerated() { out[index] = placed[rank] }
        return out
    }

    /// The radii of a dial `half` points from its middle to its edge.
    public struct Dial: Equatable, Sendable {
        /// The middle of the quarter labels, outside everything.
        public var labels: Double
        /// The line the day's moments stand on.
        public var moments: Double
        /// Where the hours' bars end at their longest.
        public var outer: Double
        /// Where they start.
        public var inner: Double
    }

    /// The radii of a dial: the quarters' names at the edge, the one line of
    /// the moments inside them, the hours' bars inside that, and the hole in
    /// the middle for the day's total. `label` is the height of a quarter's
    /// name and `mark` the width of a moment's mark.
    public static func dial(half: Double, label: Double, mark: Double) -> Dial {
        let labels = half - label / 2
        let moments = labels - label / 2 - 2 - mark / 2
        let outer = moments - mark / 2 - 3
        return Dial(labels: labels, moments: moments, outer: outer, inner: outer * 0.46)
    }

    /// The base width of a dial before the person's text size.
    public static let diameter = 300.0

    /// The width of an hour's bar at the start of the bars: most of the room
    /// between neighbours.
    public static func barWidth(inner: Double) -> Double {
        2 * Double.pi * inner / 24 * 0.6
    }

    /// What a moment says in the clock's key: its name and its time in the
    /// person's clock. A name that already holds a time is said as it is.
    public static func momentEntry(_ name: String, at: Double, uses24: Bool, locale: Locale) -> String {
        if holdsTime(name) { return name }
        return name + ", " + timeLabel(at, uses24: uses24, locale: locale)
    }

    /// Whether a moment's name already says its time (an older page wrote it
    /// in): then the time is not written beside it again.
    public static func holdsTime(_ name: String) -> Bool {
        name.range(of: #"\d:\d\d"#, options: .regularExpression) != nil
    }
}

/// The key of a chart: each kind's name and what the page says of it.
public enum KeyRules {
    /// An entry: the name, and the page's own words of its share or total
    /// when it has them.
    public static func entry(_ name: String, detail: String?) -> String {
        guard let detail, !detail.isEmpty else { return name }
        return "\(name) · \(detail)"
    }
}

/// Parts of a whole: a meter's segments, a column's parts, a strip's cells.
public enum ShareRules {
    /// Each amount's share of their total; an amount left out counts as none.
    public static func shares(_ amounts: [Double?]) -> [Double] {
        let total = amounts.reduce(0) { $0 + Swift.max($1 ?? 0, 0) }
        return amounts.map { total <= 0 ? 0 : Swift.max($0 ?? 0, 0) / total }
    }

    /// A share as a whole percentage, never 0 for a part that has some.
    public static func percent(_ share: Double) -> Int {
        share <= 0 ? 0 : Swift.max(1, Int((share * 100).rounded()))
    }

    /// Whether a share's percentage fits inside its part of a bar `width` wide,
    /// for text of `textWidth` points.
    public static func saysInside(_ share: Double, width: Double, textWidth: Double) -> Bool {
        share * width >= textWidth + 8
    }

    /// How many cells of a strip are of each category of its key, as shares
    /// of the cells that have one.
    public static func categoryShares(_ cells: [Navree.StripCell], keySize: Int) -> [Double] {
        var counts = Array(repeating: 0, count: keySize)
        for cell in cells {
            if let category = cell.category, category >= 0, category < keySize { counts[category] += 1 }
        }
        let total = counts.reduce(0, +)
        return counts.map { total == 0 ? 0 : Double($0) / Double(total) }
    }
}

/// The calendar of days: a week a column, a day a row.
public enum HeatRules {
    /// The shade of a day, 0 to 3, or `nil` for a day with no value (no data,
    /// not a day of none). `steps` are where the second, third and fourth
    /// shade begin; without them, four equal parts from 0 to the highest.
    public static func shade(_ value: Double?, steps: [Double], highest: Double) -> Int? {
        guard let value else { return nil }
        let bounds = steps.count >= 3 ? Array(steps.prefix(3)) : (1...3).map { highest * Double($0) / 4 }
        return bounds.filter { value >= $0 }.count
    }

    /// How strong each of the four shades is, as an opacity of the hue.
    public static let opacities = [0.22, 0.45, 0.7, 1.0]

    /// The first day shown: the start of the week `first` is in, by the
    /// person's first weekday.
    public static func start(_ first: Date, calendar: Calendar) -> Date {
        let day = calendar.startOfDay(for: first)
        let weekday = calendar.component(.weekday, from: day)
        let back = (weekday - calendar.firstWeekday + 7) % 7
        return calendar.date(byAdding: .day, value: -back, to: day) ?? day
    }

    /// The column (week) and row (day of the week) of a date.
    public static func cell(_ date: Date, start: Date, calendar: Calendar) -> (week: Int, day: Int) {
        let days = calendar.dateComponents([.day], from: start, to: calendar.startOfDay(for: date)).day ?? 0
        let week = days >= 0 ? days / 7 : (days - 6) / 7
        return (week, ((days % 7) + 7) % 7)
    }

    /// How many weeks the calendar spans.
    public static func weeks(first: Date, last: Date, calendar: Calendar) -> Int {
        cell(last, start: start(first, calendar: calendar), calendar: calendar).week + 1
    }

    /// The week each month's name stands over: the first week whose first
    /// day is in that month, except where the name would crowd the one
    /// before (less than `room` weeks after it).
    public static func months(first: Date, weeks: Int, calendar: Calendar, room: Int = 3) -> [(week: Int, month: Int)] {
        let start = start(first, calendar: calendar)
        var found: [(week: Int, month: Int)] = []
        var last = -1
        for week in 0..<weeks {
            guard let day = calendar.date(byAdding: .day, value: week * 7, to: start) else { continue }
            let month = calendar.component(.month, from: day)
            let starts = week == 0 || calendar.component(.day, from: day) <= 7
            if month != last, starts {
                if let previous = found.last, week - previous.week < room { found.removeLast() }
                found.append((week, month))
                last = month
            }
        }
        return found
    }

    /// A day of the calendar as the page writes it.
    public static func day(_ text: String) -> Date? {
        PagerRules.date(text)
    }
}

/// Every measure against every other.
public enum MatrixRules {
    /// The relation of two measures, either way round.
    public static func relation(_ relations: [Navree.Relation], _ a: Int, _ b: Int) -> Navree.Relation? {
        relations.first { ($0.a == a && $0.b == b) || ($0.a == b && $0.b == a) }
    }

    /// Whether a cell is drawn: with `lower`, only those under the diagonal.
    public static func drawn(row: Int, column: Int, lower: Bool) -> Bool {
        lower ? column < row : row != column
    }

    /// How strong a relation is, 0 to 1 against `peak` (1 when none is given);
    /// one that is not significant, or was never tested, is none.
    public static func strength(_ relation: Navree.Relation?, peak: Double?) -> Double {
        guard let relation, let value = relation.value, relation.significant else { return 0 }
        let top = (peak ?? 1) > 0 ? (peak ?? 1) : 1
        return Swift.min(abs(value) / top, 1)
    }
}

/// The bounds and the marks of a chart's axes.
public enum AxisRules {
    public struct Bounds: Equatable, Sendable {
        public var low: Double
        public var high: Double

        public init(_ low: Double, _ high: Double) {
            self.low = low
            self.high = high
        }
    }

    /// The value axis of a chart: the measure's own bounds where it gives
    /// them; else from zero (bars always stand on zero) to the highest value,
    /// reference or zone, rounded out to numbers read at a glance.
    public static func values(_ chart: Navree.Chart) -> Bounds {
        if chart.stacking == .shares { return Bounds(0, 1) }
        if !chart.levels.isEmpty { return Bounds(0, Double(Swift.max(chart.levels.count - 1, 1))) }
        let count = chart.series.map(\.values.count).max() ?? 0
        let stacked = chart.stacking == .values && chart.kind == .bar && chart.series.count > 1
        let tops: [Double] = stacked
            ? (0..<count).map { at in chart.series.reduce(0) { $0 + Swift.max(at < $1.values.count ? $1.values[at] ?? 0 : 0, 0) } }
            : chart.series.flatMap { $0.values.compactMap { $0 } }
        let all = tops + chart.references.compactMap(\.value) + chart.zones.flatMap { [$0.from, $0.to] }
        let lowest = all.min() ?? 0
        let highest = all.max() ?? 1
        let bars = chart.kind == .bar
        let low = chart.y?.min ?? ((bars || (lowest >= 0 && lowest < highest * 0.5)) ? 0 : lowest - (highest - lowest) * 0.15)
        let high = chart.y?.max ?? (highest + (highest - low) * 0.05)
        let round = nice(low, high > low ? high : low + 1)
        return Bounds(chart.y?.min ?? round.low, chart.y?.max ?? round.high)
    }

    /// Two ends rounded out to a step of 1, 2, 2.5 or 5 times a power of ten,
    /// about five steps apart.
    public static func nice(_ low: Double, _ high: Double) -> Bounds {
        let span = high - low
        guard span > 0 else { return Bounds(low, low + 1) }
        let step = niceStep(span / 5)
        return Bounds((low / step).rounded(.down) * step, (high / step).rounded(.up) * step)
    }

    static func niceStep(_ raw: Double) -> Double {
        let power = pow(10, (log10(raw)).rounded(.down))
        let n = raw / power
        let unit: Double = n <= 1 ? 1 : n <= 2 ? 2 : n <= 2.5 ? 2.5 : n <= 5 ? 5 : 10
        return unit * power
    }

    /// A scatter's two axes; with equal axes, one range for both.
    public static func scatter(_ plot: Navree.Scatter) -> (x: Bounds, y: Bounds) {
        let points = plot.series.flatMap(\.points)
        func padded(_ values: [Double], _ measure: Navree.Measure?) -> Bounds {
            let low = values.min() ?? 0
            let high = values.max() ?? 1
            let pad = high > low ? (high - low) * 0.08 : 1
            let round = nice(low - pad, high + pad)
            return Bounds(measure?.min ?? round.low, measure?.max ?? round.high)
        }
        let x = padded(points.map(\.x), plot.x)
        let y = padded(points.map(\.y), plot.y)
        guard plot.equalAxes else { return (x, y) }
        let both = Bounds(Swift.min(x.low, y.low), Swift.max(x.high, y.high))
        return (both, both)
    }

    /// Which of a chart's labels are written along its axis: at most `most`,
    /// a whole step apart from the first, the step a count a person reads
    /// (every 2, 3, 4, 6, 12… or 5, 7, 10…). The words are the page's (`7:15
    /// am`, `14 Oct`), so no charting library can thin them by itself.
    public static func ticks(count: Int, most: Int) -> [Int] {
        guard count > 0, most > 0 else { return [] }
        if count <= most { return Array(0..<count) }
        let steps = [2, 3, 4, 5, 6, 7, 10, 12, 14, 15, 18, 20, 24, 30, 36, 48, 60, 72, 96, 120]
        let need = Int((Double(count) / Double(most)).rounded(.up))
        let step = steps.first { $0 >= need } ?? need
        return Array(stride(from: 0, to: count, by: step))
    }

    /// How many labels fit along a plot `width` points wide, for words
    /// `label` points wide. The marks a step apart share the plot evenly, so
    /// each gap between two is the plot over their number; the first label
    /// starts at its mark and the next stands over its middle, so a gap holds
    /// a label and a half and a space. A width not yet known fits one.
    public static func most(width: Double, label: Double) -> Int {
        guard width.isFinite, label.isFinite, width > 0 else { return 1 }
        return Swift.max(1, Int(width / (Swift.max(label, 0) * 1.5 + 12)))
    }

    /// The runs of a series with no gap in them: a gap is a gap, so the line
    /// breaks there and no bar stands in it.
    public static func runs(_ values: [Double?]) -> [Int] {
        var run = 0
        var out: [Int] = []
        var inGap = false
        for value in values {
            if value == nil {
                if !inGap { run += 1 }
                inGap = true
            } else {
                inGap = false
            }
            out.append(run)
        }
        return out
    }

    /// The stretches of a series of levels: each run of one level, as the
    /// first and last place it holds (a night's stages, from the foot up).
    public static func stretches(_ values: [Double?]) -> [(level: Int, from: Int, to: Int)] {
        var out: [(level: Int, from: Int, to: Int)] = []
        for (index, value) in values.enumerated() {
            guard let value else { continue }
            let level = Int(value.rounded())
            if let last = out.last, last.level == level, last.to == index - 1 {
                out[out.count - 1].to = index
            } else {
                out.append((level, index, index))
            }
        }
        return out
    }
}

/// A progress bar's fill.
public enum ProgressRules {
    public enum Fill: Equatable, Sendable {
        /// The colour of the measure the bar belongs to.
        case measure(Navree.Colour)
        /// The colour of what the value means.
        case tone(Navree.Tone)
        /// The app's tint.
        case accent
    }

    /// The colour of the measure wins over the colour of what it means; with
    /// neither, the accent.
    public static func fill(_ progress: Navree.Progress) -> Fill {
        if let colour = progress.colour { return .measure(colour) }
        if let tone = progress.tone { return .tone(tone) }
        return .accent
    }

    /// How far along the bar is, 0 to 1 (the protocol counts in thousandths).
    public static func fraction(_ progress: Navree.Progress) -> Double? {
        progress.value.map { Swift.min(Swift.max(Double($0) / 1000, 0), 1) }
    }
}

/// Which way a trend goes, in the system's own arrows.
public enum TrendRules {
    public static func symbol(_ direction: Navree.Direction) -> String {
        switch direction {
        case .up: "arrow.up.right"
        case .down: "arrow.down.right"
        case .flat: "arrow.right"
        }
    }

    /// The colour of a trend is its news (`success`, `danger`), not its arrow:
    /// breathing faster is bad news going up. No tone: neither.
    public static func hue(_ trend: Navree.Trend) -> SystemHue {
        trend.tone.map { HueRules.system($0) } ?? .secondary
    }
}

/// The day pager: the day a screen is about, stepped in place.
/// Where a screen's day control stands.
public enum PagerPlace: Sendable, Equatable {
    /// Under the screen's large title, in the bar.
    case underTitle
    /// At the head of the screen's content, under its inline title.
    case headOfContent
}

public enum PagerRules {
    /// A day as the page writes it, `YYYY-MM-DD`, at noon UTC so no time zone
    /// moves it to another day.
    public static func date(_ text: String) -> Date? {
        let parts = text.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        var components = DateComponents()
        components.year = parts[0]
        components.month = parts[1]
        components.day = parts[2]
        components.hour = 12
        return gregorian.date(from: components)
    }

    /// The day as the core wants it in an event's `text`.
    public static func text(_ date: Date) -> String {
        let parts = gregorian.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
    }

    /// Whether a day may be chosen: inside `first` and `last` where they are given.
    public static func choosable(_ day: String, first: String?, last: String?) -> Bool {
        guard let date = date(day) else { return false }
        if let first, let low = self.date(first), date < low { return false }
        if let last, let high = self.date(last), date > high { return false }
        return true
    }

    /// The days the date control offers.
    public static func bounds(_ pager: Navree.DayPager) -> ClosedRange<Date>? {
        let low = pager.first.flatMap(date) ?? Date.distantPast
        let high = pager.last.flatMap(date) ?? Date.distantFuture
        return low <= high ? low...high : nil
    }

    /// The day control a screen shows in its bar: its own, or a pager that
    /// leads its body (a screen read from a page says it there), as a day
    /// view's date stands in the bar of the system's own apps.
    public static func inBar(_ screen: Navree.Screen) -> Navree.DayPager? {
        if let pager = screen.pager { return pager }
        if case .dayPager(let pager) = leading(screen.body) { return pager }
        return nil
    }

    /// The screen as it is drawn: a pager that leads its body taken into the
    /// bar, and out of the body, so it is drawn once.
    public static func lifted(_ screen: Navree.Screen) -> Navree.Screen {
        guard screen.pager == nil, case .stack(var stack) = screen.body,
            let index = stack.children.firstIndex(where: { !isHidden($0) }),
            case .dayPager(let pager) = stack.children[index]
        else { return screen }
        var shown = screen
        stack.children.remove(at: index)
        shown.body = .stack(stack)
        shown.pager = pager
        return shown
    }

    private static func leading(_ body: Navree.Node) -> Navree.Node? {
        if case .stack(let stack) = body { return stack.children.first { !isHidden($0) } }
        return body
    }

    private static func isHidden(_ node: Navree.Node) -> Bool {
        if case .stack(let stack) = node { return stack.hidden }
        return false
    }

    /// How the screen's title shows: as its depth says, whether or not it has
    /// a day control. The control never takes the title's place: Health and
    /// Fitness keep the screen's name in the bar, large at a destination's root.
    public static func titleMode(_ depth: ScreenDepth, pager: Navree.DayPager?) -> TitleMode {
        NavigationRules.titleMode(depth)
    }

    /// Where a screen's day control stands on iPhone and iPad: under a large
    /// title, in the bar, where the title's subtitle stands (the day it is
    /// about also written beside the title once the title folds inline); under
    /// an inline title, at the head of the content, as Health keeps a pushed
    /// chart's range control over the chart.
    public static func place(_ depth: ScreenDepth) -> PagerPlace {
        NavigationRules.titleMode(depth) == .large ? .underTitle : .headOfContent
    }

    /// The screen as it is drawn where its day control stands at the head of
    /// its content: the control the first part of the body, and out of the bar.
    public static func placed(_ screen: Navree.Screen, _ place: PagerPlace) -> Navree.Screen {
        guard place == .headOfContent, let pager = screen.pager else { return screen }
        var shown = screen
        shown.pager = nil
        if case .stack(var stack) = screen.body {
            stack.children.insert(.dayPager(pager), at: 0)
            shown.body = .stack(stack)
        } else {
            shown.body = .stack(Navree.Stack(children: [.dayPager(pager), screen.body]))
        }
        return shown
    }

    /// The day a pager shows, in words for the bar beside a folded title.
    public static func words(_ pager: Navree.DayPager) -> String {
        "\(pager.day), \(PagerWords.date(pager.date))"
    }

    static var gregorian: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        return calendar
    }
}
