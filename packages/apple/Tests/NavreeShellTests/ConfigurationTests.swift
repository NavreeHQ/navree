import Foundation
import NavreeCoreBridge
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI
import Testing

@testable import NavreeShell

@Test func aBuildWithNoTeamListsOnlyWhatAnyAppCanDo() {
    #expect(
        CoreConfiguration.caps(signedWith: [], signInScheme: nil)
            == ["clipboard", "local_notification", "open_settings", "open_url", "request_review", "share"]
    )
}

@Test func aSignInSessionIsListedOnlyWhenItCanComeBack() {
    #expect(CoreConfiguration.caps(signedWith: [], signInScheme: "charkha").contains("auth_session"))
    #expect(CoreConfiguration.caps(signedWith: [.associatedDomains], signInScheme: nil).contains("auth_session"))
}

@Test func whatNeedsATeamIsListedOnlyWhenTheBuildIsSignedForIt() {
    let none = CoreConfiguration.caps(signedWith: [], signInScheme: "charkha")
    for cap in ["confirm", "platform_sign_in", "push", "passkey"] {
        #expect(!none.contains(cap), "\(cap)")
    }
    #expect(CoreConfiguration.caps(signedWith: [.associatedDomains], signInScheme: nil).contains("confirm"))
    #expect(CoreConfiguration.caps(signedWith: [.signInWithApple], signInScheme: nil).contains("platform_sign_in"))
    #expect(CoreConfiguration.caps(signedWith: [.pushNotifications], signInScheme: nil).contains("push"))
    // A passkey's own request is never fetched by the core: no build lists it.
    #expect(!CoreConfiguration.caps(signedWith: Set(Entitlement.allCases), signInScheme: "x").contains("passkey"))
    // Listing nothing the protocol does not name.
    let known: Set<String> = [
        "clipboard", "share", "capture", "auth_session", "open_url", "push", "local_notification", "passkey", "confirm",
        "location_once", "scan_code", "app_lock", "platform_sign_in", "request_review", "open_settings",
    ]
    #expect(Set(CoreConfiguration.caps(signedWith: Set(Entitlement.allCases), signInScheme: "x")).isSubset(of: known))
}

@Test func theCoresConfigurationSaysWhatApplesSystemsDo() throws {
    let configuration = ShellConfiguration(
        origin: "https://charkha.example/", appID: "com.navree.demo", appVersion: "1.4.0", signInScheme: "charkha"
    )
    let device = DeviceTraits(
        platform: .ios, os: "27.2", form: .phone, scheme: .dark, contrast: .more, textPercent: 135, locale: "pa-IN",
        tz: "Asia/Kolkata"
    )
    let data = CoreConfiguration.json(configuration, device: device)
    let config = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    #expect(config["origin"] as? String == "https://charkha.example")
    #expect(config["app_id"] as? String == "com.navree.demo")
    #expect(config["back_to_first"] as? Bool == false)
    #expect(config["deep_links"] as? String == "routes")
    #expect(config["loopback_http"] == nil)
    #expect(config["undrawable"] as? [String] == undrawable)
    let signIn = try #require(config["sign_in"] as? [String: Any])
    #expect(signIn["native"] as? [String] == ["apple"])
    #expect(signIn["callback"] as? String == "charkha")
    let client = try #require(config["client"] as? [String: Any])
    #expect(client["platform"] as? String == "ios")
    #expect(client["form"] as? String == "phone")
    #expect(client["app"] as? String == "1.4.0")
    #expect(client["lib"] as? String == Library.version)
    #expect(client["vocab"] as? Int == 0)
    #expect(client["scheme"] as? String == "dark")
    #expect(client["contrast"] as? String == "more")
    #expect(client["text_percent"] as? Int == 135)
    #expect(client["locale"] as? String == "pa-IN")
    #expect(client["tz"] as? String == "Asia/Kolkata")
    #expect(client["caps"] as? [String] == CoreConfiguration.caps(signedWith: [], signInScheme: "charkha"))
    // The core reads it.
    #expect(Core(config: data) != nil)
}

@MainActor
@Test func aLocalServerIsAllowedOnlyWhenTheAppSaysSo() throws {
    let plain = ShellConfiguration(origin: "http://127.0.0.1:8095", appVersion: "1")
    let refused = NativeClient(core: try #require(Core(config: CoreConfiguration.json(plain, device: DeviceTraits()))))
    refused.begin()
    #expect(refused.nav.stage == .failed)
    #expect(refused.nav.failure == .config)
    let local = ShellConfiguration(origin: "http://127.0.0.1:8095", loopbackHTTP: true, appVersion: "1")
    let allowed = NativeClient(core: try #require(Core(config: CoreConfiguration.json(local, device: DeviceTraits()))))
    allowed.begin()
    #expect(allowed.nav.stage == .starting)
}

@Test func theUndrawableListIsOneWordALineInOrder() throws {
    #expect(undrawable == undrawable.sorted())
    #expect(Set(undrawable).count == undrawable.count)
    let file = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Sources/NavreeDesign/Undrawable.swift")
    let lines = try String(contentsOf: file, encoding: .utf8).split(separator: "\n")
    let words = lines.filter { $0.hasPrefix("    \"") }.map { $0.trimmingCharacters(in: .whitespaces) }
    #expect(words.count == undrawable.count)
    #expect(words.allSatisfy { $0.hasSuffix("\",") })
}

/// Every kind the drawing leaves out is named, so its page goes to the web view.
@Test func everyKindTheDrawingLeavesOutIsUndrawable() {
    let drawn: Set<String> = [
        "stack", "grid", "section", "list", "row", "split", "tabs", "disclosure", "divider", "spacer", "region", "text",
        "rich", "image", "icon", "avatar", "badge", "progress", "banner", "empty", "facts", "table", "chart", "code",
        "button", "link", "menu", "form", "field", "sheet", "dialog", "native", "gauge", "figure", "timeline", "ring",
        "rings", "day_clock", "day_pager", "meter", "columns", "strip", "range", "calendar_heatmap", "matrix_heatmap",
        "scatter", "card", "layers", "shape", "carousel", "adaptive", "toolbar", "feed_row", "message", "pane", "popover",
    ]
    let kinds: Set<String> = [
        "stack", "grid", "section", "list", "row", "split", "tabs", "disclosure", "divider", "spacer", "region", "text",
        "rich", "image", "icon", "avatar", "badge", "progress", "banner", "empty", "facts", "table", "chart", "code",
        "button", "link", "menu", "form", "field", "sheet", "dialog", "popover", "native", "web", "toolbar", "gauge",
        "figure", "timeline", "feed_row", "message", "pane", "ring", "rings", "day_clock", "day_pager", "meter",
        "columns", "strip", "range", "calendar_heatmap", "matrix_heatmap", "scatter", "card", "layers", "shape",
        "carousel", "adaptive",
    ]
    let listed = Set(undrawable)
    #expect(kinds.subtracting(drawn).isSubset(of: listed))
    // Every control is drawn: a range of days, and files sent with their bytes.
    #expect(!listed.contains { $0.hasPrefix("input:") })
}

@Test func theWindowsWidthIsCompactMediumOrExpandedFromItsSizeClassAndWidth() {
    #expect(WindowRules.width(sizeClass: .compact, points: 402) == .compact)
    #expect(WindowRules.width(sizeClass: .compact, points: 900) == .compact)
    #expect(WindowRules.width(sizeClass: .regular, points: 834) == .medium)
    #expect(WindowRules.width(sizeClass: .regular, points: 1210) == .expanded)
}

@Test func theTextSizeIsAShareOfTheDefault() {
    #expect(WindowRules.textPercent(.large) == 100)
    #expect(WindowRules.textPercent(.xSmall) == 82)
    #expect(WindowRules.textPercent(.xxxLarge) == 135)
    #expect(WindowRules.textPercent(.accessibility5) == 312)
}

@Test func onlyTheSettingsThatChangedAreTold() {
    let before = DeviceTraits(scheme: .light, contrast: .normal, textPercent: 100, locale: "en-IN", tz: "Asia/Kolkata")
    var after = before
    #expect(WindowRules.change(from: before, to: after).isEmpty)
    after.scheme = .dark
    after.textPercent = 124
    let change = WindowRules.change(from: before, to: after)
    #expect(change == CoreEvent.Environment(scheme: .dark, textPercent: 124))
}
