import Foundation

/// How large an icon drawn from its own paths stands, and how heavy its
/// lines are, so that it reads as one of the system's symbols beside the
/// same words: an icon of the toolkit's set (24 units a side, its round
/// shapes 20 units across) is as large as the system's `circle` symbol in
/// that font, its lines as heavy as the symbol's at the font's weight, and it
/// stands where the symbol stands on the line.
///
/// The numbers are the system's own, measured from SF Symbols' `circle` at
/// 100 points in every weight and image scale (iOS 26 and macOS 26 draw it
/// alike): its outer edge, its line and its frame.
public enum DrawnIcon {
    /// Where and how large a drawn icon stands, in points.
    public struct Metrics: Equatable, Sendable {
        /// The side of its square frame, as a symbol's frame.
        public let side: Double
        /// The side of the 24-unit box the toolkit's icons are drawn in.
        public let box: Double
        /// The width of its lines.
        public let stroke: Double
        /// From the frame's top to the first line's baseline: the symbol's
        /// middle stands on the middle of a capital letter.
        public let baseline: Double
    }

    /// How large a symbol is drawn against the font's size: the system's
    /// image scales.
    public enum Scale: Sendable, Equatable {
        case small, medium, large
    }

    /// The metrics of an icon beside words of `pointSize` points, of the
    /// font's `weight` (its weight trait, from -1 to 1: 0 regular, 0.3
    /// semibold, 0.4 bold), with Bold Text on or off, at an image scale;
    /// `capHeight` is the font's capital height in points.
    public static func metrics(
        pointSize: Double, weight: Double, boldText: Bool = false, scale: Scale = .medium, capHeight: Double? = nil
    ) -> Metrics {
        let shown = boldText ? bolder(weight) : weight
        let size = pointSize / 100
        let (inkShare, strokeShare, frameShare) = scaleShares(scale)
        let outer = measured(shown, \.outer) * size * inkShare
        let stroke = measured(shown, \.stroke) * size * strokeShare
        let side = measured(shown, \.frame) * size * frameShare
        // The circle's middle line is 20 units across; the box is 24.
        let box = (outer - stroke) / 20 * 24
        let cap = capHeight ?? pointSize * capShare
        return Metrics(side: side, box: box, stroke: stroke, baseline: side / 2 + cap / 2)
    }

    /// The weight Bold Text draws a symbol of `weight` in, as measured:
    /// regular becomes semibold, medium bold, semibold heavy, bold black.
    public static func bolder(_ weight: Double) -> Double {
        interpolate(weight, Self.weights, Self.bolderWeights)
    }

    /// The capital height of the system font against its size.
    static let capShare = 0.7046

    // MARK: Measured

    struct Measure {
        let outer: Double
        let stroke: Double
        let frame: Double
    }

    /// The weight traits of ultra light, thin, light, regular, medium,
    /// semibold, bold, heavy and black.
    static let weights: [Double] = [-0.8, -0.6, -0.4, 0, 0.23, 0.3, 0.4, 0.56, 0.62]

    /// `circle` at 100 points, medium scale, in each of those weights.
    static let circle: [Measure] = [
        Measure(outer: 94.5, stroke: 2.35, frame: 112),
        Measure(outer: 95.5, stroke: 3.69, frame: 113),
        Measure(outer: 98.0, stroke: 6.32, frame: 115),
        Measure(outer: 99.5, stroke: 8.31, frame: 117),
        Measure(outer: 101.0, stroke: 9.94, frame: 118),
        Measure(outer: 102.0, stroke: 11.20, frame: 119),
        Measure(outer: 103.0, stroke: 12.85, frame: 120.5),
        Measure(outer: 105.0, stroke: 15.25, frame: 122),
        Measure(outer: 106.5, stroke: 17.39, frame: 124),
    ]

    /// Each weight with Bold Text on.
    static let bolderWeights: [Double] = [-0.6, -0.4, 0, 0.3, 0.4, 0.56, 0.62, 0.62, 0.62]

    /// The small and large scales against the medium one, in a regular
    /// `circle`: its outer edge, its line and its frame.
    static func scaleShares(_ scale: Scale) -> (Double, Double, Double) {
        switch scale {
        case .small: (79.0 / 99.5, 7.47 / 8.31, 93 / 117)
        case .medium: (1, 1, 1)
        case .large: (127.0 / 99.5, 9.09 / 8.31, 150.5 / 117)
        }
    }

    static func measured(_ weight: Double, _ part: KeyPath<Measure, Double>) -> Double {
        interpolate(weight, weights, circle.map { $0[keyPath: part] })
    }

    /// The value at `x` on the line through the points (`xs`, `ys`), held at
    /// its ends.
    static func interpolate(_ x: Double, _ xs: [Double], _ ys: [Double]) -> Double {
        guard let first = xs.first, let last = xs.last else { return 0 }
        if x <= first { return ys[0] }
        if x >= last { return ys[ys.count - 1] }
        for index in 1..<xs.count where x <= xs[index] {
            let share = (x - xs[index - 1]) / (xs[index] - xs[index - 1])
            return ys[index - 1] + share * (ys[index] - ys[index - 1])
        }
        return ys[ys.count - 1]
    }

    // MARK: Bars

    /// A place that takes a destination's picture only as an image: the tab
    /// bar and the sidebar.
    public enum Bar: Sendable, Equatable {
        /// The iPhone's tab bar, and an iPad's at a compact width.
        case tabBar
        /// An iPad's sidebar.
        case sidebar
        /// A Mac's sidebar, at the person's sidebar icon size.
        case macSidebar(MacSidebarSize)
    }

    /// The sidebar icon size a Mac's General settings choose.
    public enum MacSidebarSize: Sendable, Equatable {
        case small, medium, large
    }

    /// The font size and image scale the system draws a symbol at in `bar`,
    /// so a drawn icon there is as large. Measured: the iPhone's tab bar
    /// (square.grid.2x2.fill 21 × 21, bag.fill 20.3 × 23: those of 23.4
    /// points, medium) and a Mac's sidebar at each icon size (`circle` 14,
    /// 17 and 19.5 across: 11, 13 and 15 points, large). An iPad's sidebar
    /// is taken to draw as a Mac's does, at the body's 17 points.
    public static func bar(_ bar: Bar) -> (pointSize: Double, scale: Scale) {
        switch bar {
        case .tabBar: (23.4, .medium)
        case .sidebar: (17, .large)
        case .macSidebar(.small): (11, .large)
        case .macSidebar(.medium): (13, .large)
        case .macSidebar(.large): (15, .large)
        }
    }
}
