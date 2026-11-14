import Foundation
import Testing

@testable import NavreeShell

// The icons a person may choose for the app on iPhone and iPad: only the
// build's own, the primary first, and only when there is a choice.

private let saffron = AppIcon(name: nil, label: "Saffron", preview: "AppIconPreview")
private let indigo = AppIcon(name: "AppIconIndigo", label: "Indigo", preview: "AppIconIndigoPreview")
private let green = AppIcon(name: "AppIconGreen", label: "Green", preview: "AppIconGreenPreview")

@Test func theAlternateIconsAreReadFromTheInfoPlist() {
    // The Info.plist as the asset catalog compiler writes it for a build with
    // one alternate icon.
    let info: [String: Any] = [
        "CFBundleIcons": [
            "CFBundlePrimaryIcon": ["CFBundleIconName": "AppIcon"],
            "CFBundleAlternateIcons": ["AppIconIndigo": ["CFBundleIconName": "AppIconIndigo"]],
        ]
    ]
    #expect(IconRules.declared(info) == ["AppIconIndigo"])
}

@Test func anIPadOnlyAlternateIsRead() {
    let ipad: [String: Any] = [
        "CFBundleIcons~ipad": ["CFBundleAlternateIcons": ["AppIconGreen": ["CFBundleIconName": "AppIconGreen"]]]
    ]
    #expect(IconRules.declared(ipad) == ["AppIconGreen"])
}

@Test func aBuildWithNoAlternatesDeclaresNone() {
    #expect(IconRules.declared([:]).isEmpty)
    #expect(IconRules.declared(["CFBundleIcons": ["CFBundlePrimaryIcon": ["CFBundleIconName": "AppIcon"]]]).isEmpty)
}

@Test func onlyIconsTheBuildDeclaresCanBeChosen() {
    let choosable = IconRules.choosable(configured: [saffron, indigo, green], declared: ["AppIconIndigo"])
    #expect(choosable == [saffron, indigo])
}

@Test func thePrimaryIconIsAlwaysTheFirstChoice() {
    let choosable = IconRules.choosable(configured: [indigo, saffron], declared: ["AppIconIndigo"])
    #expect(choosable.first == saffron)
    #expect(choosable == [saffron, indigo])
}

@Test func withoutThePrimaryIconNoChoiceIsOffered() {
    // The person could never come back to the app's own icon.
    #expect(IconRules.choosable(configured: [indigo], declared: ["AppIconIndigo"]).isEmpty)
}

@Test func anIconNamedTwiceIsOfferedOnce() {
    #expect(IconRules.choosable(configured: [saffron, indigo, indigo], declared: ["AppIconIndigo"]) == [saffron, indigo])
}

@Test func theMenuOffersAChoiceOnlyOfTwoIconsOrMoreOnASystemThatCanChangeThem() {
    #expect(IconRules.offersChoice([saffron, indigo], supportsAlternateIcons: true))
    #expect(!IconRules.offersChoice([saffron], supportsAlternateIcons: true))
    #expect(!IconRules.offersChoice([], supportsAlternateIcons: true))
    #expect(!IconRules.offersChoice([saffron, indigo], supportsAlternateIcons: false))
}

@Test func noAlternateMeansThePrimaryIsChosen() {
    #expect(IconRules.selected(alternateIconName: nil, icons: [saffron, indigo]) == saffron.id)
}

@Test func anAlternateIsChosenByItsName() {
    #expect(IconRules.selected(alternateIconName: "AppIconIndigo", icons: [saffron, indigo]) == indigo.id)
}

@Test func anAlternateTheAppNoLongerOffersMarksNone() {
    #expect(IconRules.selected(alternateIconName: "AppIconGreen", icons: [saffron, indigo]) == nil)
}
