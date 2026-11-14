import Testing

@testable import NavreeShell

@Test func anAddressFromOutsideGoesToTheWindowInFront() {
    #expect(OpeningRules.target(front: "B", windows: ["A", "B", "C"]) == .window("B"))
}

@Test func withNoWindowInFrontTheWindowShownLastTakesIt() {
    #expect(OpeningRules.target(front: nil, windows: ["A", "C"]) == .window("C"))
    #expect(OpeningRules.target(front: "gone", windows: ["A"]) == .window("A"))
}

@Test func withNoWindowAnAddressOpensANewOne() {
    #expect(OpeningRules.target(front: nil, windows: []) == .newWindow)
    #expect(OpeningRules.target(front: "A", windows: []) == .newWindow)
}

@Test func anAddressOfTheAppsOwnIsToldToTheCoreAsALinkFromOutside() {
    // The core selects a destination for its own address and presents any
    // other as the shell's routes say.
    #expect(OpeningRules.address("/orders") == "/orders")
    #expect(OpeningRules.address("/orders/1042?tab=items") == "/orders/1042?tab=items")
}

@Test func anAddressOfAnotherSiteOpensNothing() {
    #expect(OpeningRules.address("https://elsewhere.example/orders") == nil)
    #expect(OpeningRules.address("//elsewhere.example/orders") == nil)
    #expect(OpeningRules.address("/\\elsewhere.example") == nil)
    #expect(OpeningRules.address("orders") == nil)
    #expect(OpeningRules.address("") == nil)
}
