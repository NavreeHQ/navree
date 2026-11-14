import Foundation
import NavreeDesign
import NavreeProtocol
import SwiftUI
import Testing

// What Apple's accessibility audit asks of words, targets and text sizes,
// and how the client writes a time: each rule with the case that proves it.

/// The system's colours in the light as iOS draws them for marks and dots.
private let systemLight: [(String, ContrastRules.RGB)] = [
    ("green", ContrastRules.RGB(hex: "#34C759")!), ("orange", ContrastRules.RGB(hex: "#FF9500")!),
    ("red", ContrastRules.RGB(hex: "#FF3B30")!), ("blue", ContrastRules.RGB(hex: "#007AFF")!),
    ("yellow", ContrastRules.RGB(hex: "#FFCC00")!), ("mint", ContrastRules.RGB(hex: "#00C7BE")!),
    ("teal", ContrastRules.RGB(hex: "#30B0C7")!), ("cyan", ContrastRules.RGB(hex: "#32ADE6")!),
    ("indigo", ContrastRules.RGB(hex: "#5856D6")!), ("purple", ContrastRules.RGB(hex: "#AF52DE")!),
    ("pink", ContrastRules.RGB(hex: "#FF2D55")!),
]

@Test func theSystemsGreenAndOrangeAreTooPaleForWordsOnALightGround() {
    let white = ContrastRules.RGB(1, 1, 1)
    #expect(ContrastRules.contrast(systemLight[0].1, white) < 3)
    #expect(ContrastRules.contrast(systemLight[1].1, white) < 3)
    #expect(abs(ContrastRules.contrast(white, ContrastRules.RGB(0, 0, 0)) - 21) < 0.01)
}

@Test func wordsInAToneReadOnEveryLightGroundAndKeepTheirHue() {
    for (name, colour) in systemLight {
        let words = ContrastRules.words(colour, dark: false)
        for ground in ContrastRules.lightGrounds {
            #expect(ContrastRules.contrast(words, ground) >= ContrastRules.words, "\(name)")
        }
        // Darkened as little as reads: one step less would not.
        let shade = ContrastRules.shade(colour)
        if shade > 0 {
            let less = ContrastRules.darkened(colour, by: shade - 0.01)
            #expect(ContrastRules.lightGrounds.contains { ContrastRules.contrast(less, $0) < ContrastRules.words }, "\(name)")
        }
        // Still the same hue: darkened, never turned grey.
        let order = [colour.red, colour.green, colour.blue]
        let shaded = [words.red, words.green, words.blue]
        #expect(order.firstIndex(of: order.max()!) == shaded.firstIndex(of: shaded.max()!), "\(name)")
    }
    // In the dark the system's colours read as they are.
    #expect(ContrastRules.words(systemLight[0].1, dark: true) == systemLight[0].1)
}

@Test func anInfoButtonsTargetIsAtLeastFortyFourPoints() {
    #expect(HitRules.least == 44)
    for glyph in stride(from: HitRules.smallestGlyph, through: 60, by: 1) {
        let outset = HitRules.outset(glyph: glyph)
        #expect(glyph + 2 * outset >= HitRules.least)
        #expect(outset >= 0)
    }
    #expect(HitRules.outset(glyph: 20) == 12)
    #expect(HitRules.outset(glyph: 44) == 0)
}

@Test func aRingsNumberFollowsEveryTextSize() {
    // At the size the ring was drawn for: 0.3 of the ring, as before.
    #expect(RingRules.centreSize(diameter: 188, textScale: 1, ringScale: 1) == 188 * 0.3)
    // The ring stops growing at 1.3; the number goes on with the text.
    let sizes = [1.0, 1.3, 1.8, 2.5, 3.1].map { scale in
        RingRules.centreSize(diameter: 188 * min(scale, 1.3), textScale: scale, ringScale: min(scale, 1.3))
    }
    #expect(zip(sizes, sizes.dropFirst()).allSatisfy { $0 < $1 })
}

@Test func aTimeIsWrittenAsThePersonsLocaleWritesIt() {
    // The person's own clock, as Foundation writes it for their locale and
    // their 12 or 24 hour setting, which the app also tells the server.
    let india = Locale(identifier: "en_IN")
    let expected = Date(timeIntervalSince1970: 7 * 3600 + 7 * 60)
        .formatted(Date.FormatStyle(locale: india, timeZone: TimeZone(identifier: "UTC")!).hour().minute())
    #expect(ClockRules.timeLabel(7 + 7.0 / 60, uses24: false, locale: india) == expected)
    let twentyFour = Locale(identifier: "en_IN@hours=h23")
    #expect(ClockRules.uses24(twentyFour))
    #expect(ClockRules.timeLabel(7 + 7.0 / 60, uses24: ClockRules.uses24(twentyFour), locale: twentyFour) == "07:07")
    #expect(twentyFour.identifier(.bcp47) == "en-IN-u-hc-h23")
}
