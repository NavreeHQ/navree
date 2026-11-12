import NavreeDesign
import SwiftUI
import Testing

@Test func aMissingSizeClassIsReadAsCompact() {
    #expect(WidthClass(nil) == .compact)
    #expect(WidthClass(.compact) == .compact)
    #expect(WidthClass(.regular) == .regular)
}
