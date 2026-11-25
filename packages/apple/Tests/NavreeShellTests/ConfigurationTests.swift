import Foundation
import NavreeCoreBridge
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import SwiftUI
import Testing

@testable import NavreeShell

@Test func aBuildWithNoTeamListsOnlyWhatAnyAppCanDo() {
    var listed = ["clipboard", "local_notification", "open_settings", "open_url", "request_review"]
    // The share sheet is listed where the build shows one: on iPhone and iPad.
    #if canImport(UIKit)
    listed.append("share")
    #endif
    #expect(CoreConfiguration.caps(signedWith: [], signInScheme: nil) == listed)
    #expect(CoreConfiguration.caps(signedWith: [], signInScheme: nil).contains("share") == DeviceCalls.shares)
}

@Test func aSignInSessionIsListedOnlyWhenItCanComeBack() {
    #expect(CoreConfiguration.caps(signedWith: [], signInScheme: "charkha").contains("auth_session"))
    #expect(CoreConfiguration.caps(signedWith: [.associatedDomains], signInScheme: nil).contains("auth_session"))
}

@Test func whatNeedsATeamIsListedOnlyWhenTheBuildIsSignedForIt() {
    let none = CoreConfiguration.caps(signedWith: [], signInScheme: "charkha")
    for cap in ["confirm", "platform_sign_in", "push", "passkey", "passkey_options"] {
        #expect(!none.contains(cap), "\(cap)")
    }
    #expect(CoreConfiguration.caps(signedWith: [.associatedDomains], signInScheme: nil).contains("confirm"))
    #expect(CoreConfiguration.caps(signedWith: [.signInWithApple], signInScheme: nil).contains("platform_sign_in"))
    #expect(CoreConfiguration.caps(signedWith: [.pushNotifications], signInScheme: nil).contains("push"))
    // A passkey needs the origin as an associated domain, as a confirmation signed with one does.
    #expect(CoreConfiguration.caps(signedWith: [.associatedDomains], signInScheme: nil).contains("passkey"))
    #expect(!CoreConfiguration.caps(signedWith: [.signInWithApple, .pushNotifications], signInScheme: "x").contains("passkey"))
    // A build that lists `passkey` says with it that it shows the sheet with the options
    // the core hands over: a server offers a passkey call only to a build that lists both.
    let builds: [Set<Entitlement>] = [
        [], [.associatedDomains], [.signInWithApple, .pushNotifications], Set(Entitlement.allCases),
    ]
    for signedWith in builds {
        let caps = CoreConfiguration.caps(signedWith: signedWith, signInScheme: "x")
        #expect(caps.contains("passkey") == caps.contains("passkey_options"), "\(signedWith)")
    }
    // Listing nothing the protocol does not name: its features, and the name that says how a passkey is carried out.
    let known: Set<String> = [
        "clipboard", "share", "capture", "auth_session", "open_url", "push", "local_notification", "passkey", "confirm",
        "location_once", "scan_code", "app_lock", "platform_sign_in", "request_review", "open_settings", "passkey_options",
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
    // A caller that gives no secret says none, and asks for no key from before.
    #expect(config["secret"] == nil)
    #expect(config["old_key"] == nil)
}

@Test func theCoresConfigurationCarriesTheInstallsSecretAndAsksForAPlainKeyOnlyWhileOneIsKept() throws {
    let configuration = ShellConfiguration(origin: "https://charkha.example", appID: "com.navree.demo", appVersion: "1.4.0")
    let secret = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
    func config(secret: String?, oldKey: Bool) throws -> [String: Any] {
        let data = CoreConfiguration.json(configuration, device: DeviceTraits(), secret: secret, oldKey: oldKey)
        return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }
    let given = try config(secret: secret, oldKey: false)
    #expect(given["secret"] as? String == secret)
    #expect(given["old_key"] == nil)
    let kept = try config(secret: secret, oldKey: true)
    #expect(kept["secret"] as? String == secret)
    #expect(kept["old_key"] as? Bool == true)
    // With no secret the core gives plain keys, and there is no key from before to ask for.
    let none = try config(secret: nil, oldKey: true)
    #expect(none["secret"] == nil)
    #expect(none["old_key"] == nil)
    // The core reads a configuration with a secret, and makes the person's key with it.
    #expect(Core(config: CoreConfiguration.json(configuration, device: DeviceTraits(), secret: secret)) != nil)
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
