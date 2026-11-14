import Foundation
import NavreeDesign
import SwiftUI
import Testing

// An icon drawn from its own paths stands as large as the system's symbols
// beside the same words, its lines as heavy, at every text style, weight,
// image scale and with Bold Text.

/// SF Symbols' `circle` as measured on this Mac (macOS 27.2) at the system
/// font's sizes for each text style, regular, bold and with Bold Text: its
/// outer edge, its line and its frame, in points.
private let measuredCircle: [(style: String, size: Double, weight: Double, boldText: Bool, outer: Double, stroke: Double, side: Double)] = [
    ("caption", 10, 0, false, 10.0, 0.83, 11.75),
    ("caption", 10, 0.4, false, 10.25, 1.29, 12.12),
    ("subheadline", 11, 0, false, 11.0, 0.91, 13.0),
    ("subheadline", 11, 0.4, false, 11.38, 1.41, 13.38),
    ("body", 13, 0, false, 13.0, 1.08, 15.0),
    ("body", 13, 0.4, false, 13.38, 1.67, 15.75),
    ("body", 13, 0, true, 13.25, 1.46, 15.5),
    ("title3", 15, 0, false, 14.88, 1.25, 17.5),
    ("title3", 15, 0.4, false, 15.5, 1.93, 18.0),
]

@Test func aDrawnIconIsAsLargeAndAsHeavyAsTheSystemsCircleAtEachTextStyle() {
    for circle in measuredCircle {
        let metrics = DrawnIcon.metrics(pointSize: circle.size, weight: circle.weight, boldText: circle.boldText)
        // The toolkit's circle: 20 units across its middle line, in a 24-unit box.
        let outer = metrics.box / 24 * 20 + metrics.stroke
        #expect(abs(outer - circle.outer) < 0.3, "\(circle.style) outer \(outer) against \(circle.outer)")
        #expect(abs(metrics.stroke - circle.stroke) < 0.1, "\(circle.style) line \(metrics.stroke) against \(circle.stroke)")
        #expect(abs(metrics.side - circle.side) < 0.6, "\(circle.style) frame \(metrics.side) against \(circle.side)")
    }
}

@Test func aDrawnIconFollowsTheTextSizeOfEachStyleOnAnIPhone() {
    // iOS's sizes at the default text size and at the largest accessibility
    // size: caption 12 and 40, footnote 13 and 44, subheadline 15 and 49,
    // body 17 and 53, title3 20 and 58.
    for size in [12.0, 13, 15, 17, 20, 40, 44, 49, 53, 58] {
        let metrics = DrawnIcon.metrics(pointSize: size, weight: 0)
        let outer = metrics.box / 24 * 20 + metrics.stroke
        #expect(abs(outer - size * 0.995) < 0.05 * size / 10)
        #expect(abs(metrics.stroke - size * 0.0831) < 0.01 * size / 10)
        // The old drawing: a 20-point box at the body's 17 points, scaled
        // alike, a 2-unit line.
        let old = 20 * size / 17
        #expect(metrics.box < old)
        #expect(metrics.stroke < 2 * old / 24)
    }
}

@Test func boldTextDrawsADrawnIconInTheWeightItDrawsASymbol() {
    #expect(DrawnIcon.bolder(0) == 0.3)
    #expect(DrawnIcon.bolder(0.23) == 0.4)
    #expect(DrawnIcon.bolder(0.3) == 0.56)
    #expect(DrawnIcon.bolder(0.4) == 0.62)
    #expect(DrawnIcon.bolder(0.62) == 0.62)
    let regular = DrawnIcon.metrics(pointSize: 17, weight: 0)
    let bold = DrawnIcon.metrics(pointSize: 17, weight: 0, boldText: true)
    #expect(bold.stroke > regular.stroke)
    #expect(bold.stroke == DrawnIcon.metrics(pointSize: 17, weight: 0.3).stroke)
}

@Test func aDrawnIconFollowsTheImageScale() {
    let small = DrawnIcon.metrics(pointSize: 100, weight: 0, scale: .small)
    let medium = DrawnIcon.metrics(pointSize: 100, weight: 0, scale: .medium)
    let large = DrawnIcon.metrics(pointSize: 100, weight: 0, scale: .large)
    // `circle` at 100 points: 79, 99.5 and 127 across; frames 93, 117, 150.5.
    #expect(abs(small.box / 24 * 20 + small.stroke - 79) < 0.5)
    #expect(abs(medium.box / 24 * 20 + medium.stroke - 99.5) < 0.5)
    #expect(abs(large.box / 24 * 20 + large.stroke - 127) < 0.5)
    #expect(abs(small.side - 93) < 0.5 && abs(large.side - 150.5) < 0.5)
}

@Test func aDrawnIconStandsOnTheLineWhereASymbolStands() {
    // The symbol's middle on the middle of a capital: at 13 points (capital
    // 9.16) a 15-point frame's baseline is 12.08 from its top, as measured
    // (the circle's ink from 2.0 to 15.0 on a line whose baseline is at 13).
    let metrics = DrawnIcon.metrics(pointSize: 13, weight: 0, capHeight: 9.16)
    #expect(abs(metrics.baseline - (metrics.side / 2 + 4.58)) < 0.01)
    #expect(abs(metrics.baseline - 12.08) < 0.2)
}

@Test func aDrawnIconInABarIsAsLargeAsTheSystemsSymbolsThere() {
    // A Mac's sidebar at the medium icon size draws a symbol as 13 points at
    // the large scale: measured square.grid.2x2 15 × 15, bag 14.5 × 16.5,
    // tray 19.5 × 14, which are those of 13 points, large.
    #expect(DrawnIcon.bar(.macSidebar(.medium)).pointSize == 13)
    #expect(DrawnIcon.bar(.macSidebar(.medium)).scale == .large)
    // The iPhone's tab bar: square.grid.2x2.fill 21 × 21 and bag.fill
    // 20.3 × 23, those of 23.4 points, medium.
    #expect(DrawnIcon.bar(.tabBar).pointSize == 23.4)
    let tab = DrawnIcon.metrics(pointSize: 23.4, weight: 0)
    #expect(abs(tab.box / 24 * 20 + tab.stroke - 23.3) < 0.2)
    // The old image: a 24-point box with a 20-point icon in it in every bar.
    let mac = DrawnIcon.metrics(pointSize: 13, weight: 0, scale: .large)
    #expect(mac.box < 20)
}

#if canImport(AppKit)
import AppKit
import CoreText
import NavreeProtocol

/// What a view draws: the box of its ink, its line across the middle row
/// from the left, its frame, in points.
@MainActor
private func ink<V: View>(_ view: V) -> (box: CGRect, stroke: Double, frame: CGSize) {
    let scale = 8.0
    let renderer = ImageRenderer(content: view.foregroundStyle(.black).environment(\.colorScheme, .light))
    renderer.scale = scale
    guard let image = renderer.cgImage else { return (.zero, 0, .zero) }
    let width = image.width, height = image.height
    var data = [UInt8](repeating: 0, count: width * height * 4)
    let context = CGContext(
        data: &data, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    func alpha(_ x: Int, _ y: Int) -> UInt8 { data[(y * width + x) * 4 + 3] }
    var minX = width, minY = height, maxX = -1, maxY = -1
    for y in 0..<height {
        for x in 0..<width where alpha(x, y) > 127 {
            minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
        }
    }
    guard maxX >= 0 else { return (.zero, 0, CGSize(width: Double(width) / scale, height: Double(height) / scale)) }
    let middle = (minY + maxY) / 2
    var line = 0.0
    var x = minX
    while x < width, alpha(x, middle) > 0 {
        line += Double(alpha(x, middle)) / 255
        x += 1
    }
    return (
        CGRect(
            x: Double(minX) / scale, y: Double(minY) / scale,
            width: Double(maxX - minX + 1) / scale, height: Double(maxY - minY + 1) / scale
        ),
        line / scale,
        CGSize(width: Double(width) / scale, height: Double(height) / scale)
    )
}

/// The toolkit's `circle`, as the server sends its paths.
private let circlePaths = ["M2 12a10 10 0 1 0 20 0a10 10 0 1 0 -20 0"]

@MainActor
@Test func aDrawnCircleMatchesTheSystemsCircleBesideWordsOfEachStyle() {
    let styles: [(String, Font.TextStyle)] = [
        ("caption", .caption), ("footnote", .footnote), ("subheadline", .subheadline), ("body", .body),
        ("headline", .headline), ("title3", .title3),
    ]
    for (name, style) in styles {
        for weight in [Font.Weight.regular, .bold] {
            for boldText in [false, true] {
                let legibility: LegibilityWeight = boldText ? .bold : .regular
                let symbol = ink(
                    Image(systemName: "circle").font(.system(style)).fontWeight(weight)
                        .environment(\.legibilityWeight, legibility)
                )
                let drawn = ink(
                    IconShapeView(paths: circlePaths, fills: [], viewBox: "0 0 24 24").font(.system(style))
                        .fontWeight(weight).environment(\.legibilityWeight, legibility)
                )
                let label = "\(name) \(weight == .bold ? "bold" : "regular")\(boldText ? " with Bold Text" : "")"
                #expect(abs(drawn.box.width - symbol.box.width) <= 1, "\(label): \(drawn.box.width) against \(symbol.box.width)")
                #expect(abs(drawn.box.height - symbol.box.height) <= 1, "\(label)")
                #expect(abs(drawn.stroke - symbol.stroke) <= 0.25, "\(label): line \(drawn.stroke) against \(symbol.stroke)")
                #expect(abs(drawn.frame.width - symbol.frame.width) <= 1, "\(label): frame \(drawn.frame) against \(symbol.frame)")
            }
        }
    }
}

@MainActor
@Test func aDrawnIconStandsOnTheLineOfTheWordsBesideIt() {
    for style in [Font.TextStyle.caption, .subheadline, .body, .title3] {
        let symbol = ink(
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text("H").hidden()
                Image(systemName: "circle")
            }
            .font(.system(style))
        )
        let drawn = ink(
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text("H").hidden()
                IconShapeView(paths: circlePaths, fills: [], viewBox: "0 0 24 24")
            }
            .font(.system(style))
        )
        let words = ink(Text("H").font(.system(style)))
        // Both rows are as tall as the words' line; the circle's middle
        // stands at the same height in each.
        #expect(abs(drawn.box.midY - symbol.box.midY) <= 0.5, "\(style): \(drawn.box.midY) against \(symbol.box.midY)")
        #expect(drawn.frame.height <= max(symbol.frame.height, words.frame.height) + 0.5, "\(style)")
    }
}

@Test func theRupeeInFiguresIsTheSystemFontsOwnInEveryDesignAndWeight() {
    // A figure's ₹ (big, rounded, monospaced digits, a chart's axis) is drawn
    // by the font of its digits, never by a font the system falls back to.
    for design in [NSFontDescriptor.SystemDesign.default, .rounded, .monospaced] {
        for weight in [NSFont.Weight.regular, .medium, .semibold, .bold, .heavy] {
            let base = NSFont.systemFont(ofSize: 34, weight: weight)
            guard let descriptor = base.fontDescriptor.withDesign(design), let font = NSFont(descriptor: descriptor, size: 34)
            else {
                Issue.record("no \(design.rawValue) system font")
                continue
            }
            var characters: [UniChar] = [0x20B9]
            var glyphs: [CGGlyph] = [0]
            #expect(CTFontGetGlyphsForCharacters(font as CTFont, &characters, &glyphs, 1), "\(font.fontName)")
            let drawnWith = CTFontCreateForString(font as CTFont, "₹" as CFString, CFRange(location: 0, length: 1))
            #expect(CTFontCopyPostScriptName(drawnWith) as String == font.fontName, "\(font.fontName)")
        }
    }
    let figures = NSFont.monospacedDigitSystemFont(ofSize: 34, weight: .semibold)
    var characters: [UniChar] = [0x20B9]
    var glyphs: [CGGlyph] = [0]
    #expect(CTFontGetGlyphsForCharacters(figures as CTFont, &characters, &glyphs, 1))
}

@MainActor
@Test func aDrawnIconInAMacSidebarIsAsLargeAsTheSymbolsThere() throws {
    let icon = Navree.Icon(name: "toolkit-circle", paths: circlePaths, viewBox: "0 0 24 24")
    let image = try #require(IconImage.image(icon, in: .macSidebar(.medium)))
    let drawn = ink(image)
    let symbol = ink(Image(systemName: "circle").font(.system(size: 13)).imageScale(.large))
    #expect(abs(drawn.box.width - symbol.box.width) <= 1, "\(drawn.box.width) against \(symbol.box.width)")
    #expect(abs(drawn.stroke - symbol.stroke) <= 0.25, "\(drawn.stroke) against \(symbol.stroke)")
    #expect(abs(drawn.frame.width - symbol.frame.width) <= 1)
}
#endif
