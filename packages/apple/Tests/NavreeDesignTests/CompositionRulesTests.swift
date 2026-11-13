import Foundation
import NavreeDesign
import NavreeProtocol
import SwiftUI
import Testing

// The rules a composed screen is set out by: the room and its margins, the
// stacks that turn, the grids, the cards, the carousels, the words and the
// motion, each with the case that proves it.

private func text(_ value: String, align: Navree.Align? = nil) -> Navree.Node {
    .text(Navree.Text(align: align, value: value))
}

private let navigate = Navree.Action.navigate(Navree.Action.Navigate(href: "/sleep"))

// MARK: The screen and its room

@Test func aScreenWithCardsIsComposedAndAListOfSectionsIsNot() {
    let card = Navree.Node.card(Navree.Card(children: [text("a")]))
    #expect(CompositionRules.isComposed(.stack(Navree.Stack(children: [card]))))
    #expect(CompositionRules.isComposed(.stack(Navree.Stack(children: [], wide: .horizontal))))
    #expect(CompositionRules.isComposed(.stack(Navree.Stack(children: [], pad: 2))))
    #expect(!CompositionRules.isComposed(.stack(Navree.Stack(children: [.section(Navree.Section(children: [text("a")]))]))))
}

@Test func theContentStartsAtTheSystemsMarginSoEverythingSharesOneLeadingEdge() {
    // A phone held upright: 16 points, as the bar's title and back button.
    #expect(CompositionRules.margin(width: 402) == 16)
    #expect(CompositionRules.room(width: 402) == 370)
    // A large phone, a tablet, a window: 20.
    #expect(CompositionRules.margin(width: 440) == 20)
    #expect(CompositionRules.margin(width: 834) == 20)
    #expect(CompositionRules.room(width: 834) == 794)
    #expect(CompositionRules.room(width: 10) == 0)
}

@Test func aRoomIsWideByItsOwnWidthAndByTheWindowsClassOnlyWhenItsWidthIsUnknown() {
    // The content of a phone held upright is never wide, whatever the window says.
    #expect(!CompositionRules.isWide(room: 370, width: .regular))
    // A tablet's content is.
    #expect(CompositionRules.isWide(room: 794, width: .compact))
    // The line is the same as Android's.
    #expect(CompositionRules.isWide(room: 600, width: .compact))
    #expect(!CompositionRules.isWide(room: 599, width: .regular))
    // Unknown: the window's class.
    #expect(CompositionRules.isWide(room: nil, width: .regular))
    #expect(!CompositionRules.isWide(room: nil, width: .compact))
}

// MARK: Stacks

@Test func aStackTurnsAcrossOnlyInAWideRoomAndAnAdaptiveDrawsOneRoom() {
    let stack = Navree.Stack(axis: .vertical, children: [], wide: .horizontal)
    #expect(CompositionRules.axis(stack, wide: true) == .horizontal)
    #expect(CompositionRules.axis(stack, wide: false) == .vertical)
    #expect(CompositionRules.axis(Navree.Stack(axis: .horizontal, children: []), wide: true) == .horizontal)
    let adaptive = Navree.Adaptive(compact: [.divider(Navree.Divider())], wide: [])
    #expect(CompositionRules.room(adaptive, wide: false).count == 1)
    #expect(CompositionRules.room(adaptive, wide: true).isEmpty)
}

@Test func aGridACarouselAndAChartTakeTheRoomLeftAndARingKeepsItsSize() {
    #expect(CompositionRules.grows(.stack(Navree.Stack(children: [], grow: true))))
    #expect(!CompositionRules.grows(.stack(Navree.Stack(children: []))))
    #expect(CompositionRules.grows(.layers(Navree.Layers(grow: true))))
    #expect(CompositionRules.grows(.grid(Navree.Grid())))
    #expect(CompositionRules.grows(.carousel(Navree.Carousel())))
    #expect(CompositionRules.grows(.chart(Navree.Chart(kind: .line))))
    #expect(!CompositionRules.grows(.ring(Navree.Ring(max: 100, value: 80))))
    #expect(!CompositionRules.grows(.shape(Navree.Shape())))
    #expect(!CompositionRules.grows(.layers(Navree.Layers(size: Navree.Size(height: 40, width: 40)))))
}

@Test func aCentringColumnTakesTheWidthAndNeverTheHeightOfWhatStandsBesideIt() {
    let centred = Navree.Stack(align: .center, children: [.ring(Navree.Ring(max: 100, value: 80))])
    // On its own line, it is as wide as its place, so the ring stands in the middle.
    #expect(CompositionRules.fillsWidth(centred, axis: .vertical, inLine: false))
    // In a line of parts (the lead in a wide room), it keeps the width it needs.
    #expect(!CompositionRules.fillsWidth(centred, axis: .vertical, inLine: true))
    // A column from the start keeps its own width; a column that grows in a line takes the room left.
    #expect(!CompositionRules.fillsWidth(Navree.Stack(children: []), axis: .vertical, inLine: false))
    #expect(CompositionRules.fillsWidth(Navree.Stack(children: [], grow: true), axis: .vertical, inLine: true))
    // In a column, a part that grows takes the width, so its words start at the edge.
    #expect(CompositionRules.fillsWidth(Navree.Stack(children: [], grow: true), axis: .vertical, inLine: false))
    // A line that centres is not a column.
    #expect(!CompositionRules.fillsWidth(Navree.Stack(align: .center, axis: .horizontal, children: []), axis: .horizontal, inLine: false))
}

@Test func aStacksGapIsInStepsAndNoneIsTheSystemsOwn() {
    #expect(CompositionRules.spacing(0) == nil)
    #expect(CompositionRules.spacing(3) == 12)
    #expect(CompositionRules.points(steps: 5) == 20)
}

// MARK: Grids

@Test func aGridOfFiveWithNoLeastWidthIsOneRowOfFiveWhileItsNamesFitWhole() {
    // The pillars at a phone's width: five, as many as the grid says.
    #expect(CompositionRules.columns(width: 370, cols: 5, min: nil, gap: 8, parts: 5) == 5)
    // Names that need 56 points fit five cells of 67.6: the row stays five.
    let need = { (name: Double) in { (count: Int) in CompositionRules.columnWidth(width: 370, count: count, gap: 8) < name } }
    #expect(CompositionRules.wholeColumns(most: 5, cuts: need(56)) == 5)
    // At the largest text sizes the longest name needs 130: two columns, no name cut or shrunk.
    #expect(CompositionRules.wholeColumns(most: 5, cuts: need(130)) == 2)
    // A name wider than the whole row: one column, the least there is.
    #expect(CompositionRules.wholeColumns(most: 5, cuts: need(500)) == 1)
    #expect(CompositionRules.wholeColumns(most: 0, cuts: { _ in true }) == 1)
}

@Test func aRowOfTilesShrinksAsOneAndNeverAtTheAccessibilitySizes() {
    #expect(CompositionRules.rowTypeSizes(.large) == [.large, .medium, .small])
    #expect(CompositionRules.rowTypeSizes(.xxxLarge) == [.xxxLarge, .xxLarge, .xLarge])
    #expect(CompositionRules.rowTypeSizes(.xSmall) == [.xSmall])
    #expect(CompositionRules.rowTypeSizes(.small) == [.small, .xSmall])
    #expect(CompositionRules.rowTypeSizes(.accessibility2) == [.accessibility2])
}

@Test func aGridWithALeastWidthDrawsAsManyAsFitAndNoMoreThanItsCount() {
    // Five tiles of at least 96 points at a phone's width: three to a row, names whole.
    #expect(CompositionRules.columns(width: 370, cols: 5, min: 96, gap: 12, parts: 5) == 3)
    // At a tablet's width all five.
    #expect(CompositionRules.columns(width: 794, cols: 5, min: 96, gap: 12, parts: 5) == 5)
    // Figures of at least 150: two in a card on a phone, four across a tablet.
    #expect(CompositionRules.columns(width: 338, cols: nil, min: 150, gap: 12, parts: 8) == 2)
    #expect(CompositionRules.columns(width: 794, cols: nil, min: 150, gap: 12, parts: 8) == 4)
    // Never more columns than parts.
    #expect(CompositionRules.columns(width: 794, cols: nil, min: 150, gap: 12, parts: 3) == 3)
    // Larger text: the least width grows with it.
    #expect(CompositionRules.columns(width: 370, cols: 5, min: 96, gap: 12, textScale: 2, parts: 5) == 1)
    // Neither a count nor a least width: columns of 160.
    #expect(CompositionRules.columns(width: 370, cols: nil, min: nil, gap: 12, parts: 4) == 2)
    // Never fewer than one.
    #expect(CompositionRules.columns(width: 40, cols: nil, min: 150, gap: 12, parts: 4) == 1)
    // A place with no bound holds them all, up to the count; a part with no width too.
    #expect(CompositionRules.columns(width: .infinity, cols: 5, min: 96, gap: 12, parts: 5) == 5)
    #expect(CompositionRules.columns(width: 370, cols: nil, min: 0, gap: 0, parts: 5) == 5)
}

@Test func aGridsColumnsShareItsWidthLessTheGaps() {
    #expect(CompositionRules.columnWidth(width: 370, count: 5, gap: 8) == 67.6)
    #expect(CompositionRules.columnWidth(width: 370, count: 1, gap: 8) == 370)
}

@Test func tilesOfARowShareOneTopAndOneFootAndOtherPartsKeepTheirHeight() {
    #expect(CompositionRules.fillsRow(.card(Navree.Card())))
    #expect(!CompositionRules.fillsRow(.figure(Navree.Figure(label: "Steps", value: "8,324"))))
    #expect(!CompositionRules.fillsRow(.link(Navree.Link(action: navigate, label: "Sleep"))))
}

// MARK: Cards and sections

@Test func aSectionOfOneCardIsOneGroupWithOneSurface() {
    let group = Navree.Section(children: [.card(Navree.Card())])
    #expect(CompositionRules.isGroup(group))
    #expect(!CompositionRules.isGroup(Navree.Section(children: [.card(Navree.Card()), .divider(Navree.Divider())])))
    // A group's surface is its card: the section itself stands on none.
    #expect(!CompositionRules.standsOnSurface(.section(group)))
    #expect(!CompositionRules.standsOnSurface(.section(Navree.Section(children: [.card(Navree.Card())], style: .inset))))
    // A list on the ground has one, as an inset grouped list; so has an inset section.
    #expect(CompositionRules.standsOnSurface(.list(Navree.List())))
    #expect(CompositionRules.standsOnSurface(.section(Navree.Section(children: [text("a")], style: .inset))))
    #expect(!CompositionRules.standsOnSurface(.section(Navree.Section(children: [text("a")]))))
}

@Test func aCardsPaddingIsItsStepsOrTheSystemsAndATileKeepsRoomForItsWords() {
    #expect(CompositionRules.padding(Navree.Card()) == 16)
    #expect(CompositionRules.padding(Navree.Card(pad: 3)) == 12)
    #expect(CompositionRules.padding(Navree.Card(), columns: 3) == 12)
    #expect(CompositionRules.padding(Navree.Card(), columns: 5) == 12)
    #expect(CompositionRules.padding(Navree.Card(pad: 4), columns: 5) == 16)
    #expect(CompositionRules.padding(Navree.Card(), columns: 2) == 16)
    #expect(CompositionRules.cardCorner == 16)
    #expect(CompositionRules.cardSpacing == 8)
}

// MARK: The card that grows into its screen

@Test func onlyACardThatSaysZoomHasAnIdAndNavigatesIsAZoomSource() {
    let card = Navree.Card(action: navigate, id: "last-night-card", transition: .zoom)
    #expect(ZoomRules.source(card) == "last-night-card")
    // No id, no source.
    #expect(ZoomRules.source(Navree.Card(action: navigate, transition: .zoom)) == nil)
    // A tap that fills a sheet of the same screen opens nothing to grow into.
    let request = Navree.Action.request(Navree.Action.Request(href: "/inventory/7/peek"))
    #expect(ZoomRules.source(Navree.Card(action: request, id: "shelf-7", transition: .zoom)) == nil)
    // A card that does not say it.
    #expect(ZoomRules.source(Navree.Card(action: navigate, id: "activity-card")) == nil)
    // Less motion: the screen comes in as any screen does.
    #expect(ZoomRules.source(card, reduceMotion: true) == nil)
}

@Test func aScreenGrowsOutOfThePartItWasOpenedFromOnlyWhenThatPartIsAZoomCard() {
    let card = Navree.Node.card(Navree.Card(action: navigate, id: "last-night-card", transition: .zoom))
    let pillar = Navree.Node.link(Navree.Link(action: navigate, children: [text("Sleep")], id: "pillar-sleep", label: "Sleep"))
    let home = Navree.Node.stack(Navree.Stack(children: [.section(Navree.Section(children: [card])), pillar]))
    #expect(ZoomRules.source(from: "last-night-card", under: home) == "last-night-card")
    // Opened from a link: a plain push.
    #expect(ZoomRules.source(from: "pillar-sleep", under: home) == nil)
    // Opened from nothing, or from a part the screen under it no longer holds.
    #expect(ZoomRules.source(from: nil, under: home) == nil)
    #expect(ZoomRules.source(from: "gone", under: home) == nil)
    #expect(ZoomRules.source(from: "last-night-card", under: nil) == nil)
    #expect(ZoomRules.source(from: "last-night-card", under: home, reduceMotion: true) == nil)
    // Among the screens the runtime holds, the one that has the card.
    let sleep = Navree.Node.stack(Navree.Stack(children: [text("Sleep")]))
    #expect(ZoomRules.source(from: "last-night-card", among: [sleep, home]) == "last-night-card")
    #expect(ZoomRules.source(from: "pillar-sleep", among: [sleep, home]) == nil)
    #expect(ZoomRules.source(from: nil, among: [home]) == nil)
}

// MARK: Carousels

@Test func aCarouselShowsAsManyAsFitEachTheSameWidthAndPeeksAtTheNext() {
    // Three cards of at least 300 on a phone's ground (margin 16, gap 12, the
    // cards' words 16 in): one shows across the content, and the next starts
    // in the margin past the gap, 4 points of its edge showing.
    let phone = CompositionRules.carousel(room: 370, min: 300, gap: 12, parts: 3, margin: 16, edge: 16)
    #expect(phone.shown == 1)
    #expect(phone.width == 370)
    #expect(CompositionRules.scrolls(shown: phone.shown, parts: 3))
    // At a tablet's width two show, ending with the content above them, and
    // the third shows 8 points of its edge: never its words, which start 16 in.
    let tablet = CompositionRules.carousel(room: 794, min: 300, gap: 12, parts: 3, margin: 20, edge: 16)
    #expect(tablet.shown == 2)
    #expect(tablet.width == 391)
    #expect(CompositionRules.carouselPeek(margin: 20, gap: 12, edge: 16) == 8)
    #expect(CompositionRules.carouselPeek(margin: 20, gap: 12, edge: 16) < 16)
    // Inside a surface (no margin to reach into) the edge peeks inside the room.
    let inside = CompositionRules.carousel(room: 338, min: 300, gap: 12, parts: 3, edge: 16)
    #expect(inside.shown == 1)
    #expect(inside.width == 310)
    // Wide margins show no more than the edge.
    #expect(CompositionRules.carouselPeek(margin: 40, gap: 12, edge: 16) == 16)
    // Parts with no surface do not peek: the next starts at the window's edge.
    #expect(CompositionRules.carouselPeek(margin: 16, gap: 12, edge: 0) == 0)
    #expect(CompositionRules.carousel(room: 370, min: 300, gap: 12, parts: 3, margin: 16).width == 374)
    // A card's padding is its edge; a part with no surface has none.
    let card = Navree.Node.card(Navree.Card(children: [text("Revenue")]))
    #expect(CompositionRules.surfaceEdge([card, card]) == 16)
    #expect(CompositionRules.surfaceEdge([card, text("Orders")]) == 0)
    // When all fit, each takes its share and nothing scrolls.
    let wide = CompositionRules.carousel(room: 1000, min: 300, gap: 12, parts: 3)
    #expect(wide.shown == 3)
    #expect(!CompositionRules.scrolls(shown: wide.shown, parts: 3))
    // Fewer parts than fit: they share the whole room.
    #expect(CompositionRules.carousel(room: 794, min: 300, gap: 12, parts: 1).width == 794)
    // No least width: each part has its own (chips).
    #expect(CompositionRules.carousel(room: 370, min: nil, gap: 8, parts: 6).width == nil)
}

// MARK: Words, symbols and hints

@Test func aWordOrAFigureStaysWholeAndASentenceWraps() {
    #expect(CompositionRules.keepsWhole(Navree.Text(value: "1,994")))
    #expect(CompositionRules.keepsWhole(Navree.Text(align: .center, value: "Movement")))
    #expect(!CompositionRules.keepsWhole(Navree.Text(value: "25% of score")))
    #expect(!CompositionRules.keepsWhole(Navree.Text(value: "  ")))
    #expect(CompositionRules.keepsWhole(Navree.Text(value: "₹14,36,473")))
    #expect(!CompositionRules.keepsWhole(Navree.Text(value: "harpreet.singh@example.in")))
}

@Test func aDrawnIconIsAsLargeAsASymbolOfItsStyle() {
    #expect(CompositionRules.iconSide(nil) == 20)
    #expect(CompositionRules.iconSide(.body) == 20)
    #expect(CompositionRules.iconSide(.caption) < CompositionRules.iconSide(.body))
    #expect(CompositionRules.iconSide(.title2) < CompositionRules.iconSide(.title))
    #expect(CompositionRules.iconSide(.display) > CompositionRules.iconSide(.largeTitle))
}

@Test func aHintIsOfferedByWhatItIsAbout() {
    #expect(HintRules.actionName(Navree.Hint(about: "the health score", text: "…")) == "About the health score")
    #expect(HintRules.actionName(Navree.Hint(about: " ", text: "…")) == "About")
}

// MARK: Motion

@Test func aCountRollsAFadeFadesAndLessMotionOnlyFades() {
    #expect(CompositionRules.change(.count, reduceMotion: false) == .rolls)
    #expect(CompositionRules.change(.count, reduceMotion: true) == .fades)
    #expect(CompositionRules.change(.fade, reduceMotion: false) == .fades)
    #expect(CompositionRules.change(nil, reduceMotion: false) == .none)
}

@Test func aPartComesInAsItSaysAndOnlyFadesForLessMotion() {
    let rising = Navree.Node.card(Navree.Card(appear: .rise))
    #expect(CompositionRules.arrival(rising, reduceMotion: false) == .rises)
    #expect(CompositionRules.arrival(rising, reduceMotion: true) == .fades)
    #expect(CompositionRules.arrival(.stack(Navree.Stack(appear: .scale, children: [])), reduceMotion: false) == .grows)
    #expect(CompositionRules.arrival(.stack(Navree.Stack(appear: .fade, children: [])), reduceMotion: true) == .fades)
    #expect(CompositionRules.arrival(.card(Navree.Card()), reduceMotion: false) == .none)
    #expect(CompositionRules.arrival(text("a"), reduceMotion: false) == .none)
}

@Test func aPartKeepsItsIdentityWhenAnotherIsAddedBeforeIt() {
    let a = Navree.Node.card(Navree.Card(id: "a"))
    let b = Navree.Node.card(Navree.Card(id: "b"))
    let before = CompositionRules.identities([a, b])
    let after = CompositionRules.identities([.card(Navree.Card(id: "new")), a, b])
    #expect(before == ["#a", "#b"])
    #expect(after == ["#new", "#a", "#b"])
    // Parts with no id are known by their place; the same id twice stays two parts.
    #expect(CompositionRules.identities([text("x"), a, a]) == ["@0", "#a", "#a'"])
}
