import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

#if canImport(AppKit)
import AppKit
#elseif canImport(UIKit)
import UIKit
#endif

// The client's own table of system symbols, read after the protocol's: every
// icon the recorded answers, the corpus and the core's transcripts hold has a
// symbol, and every symbol is the system's, on iOS 26 and macOS 26.

/// The repository's root.
private let root = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()

/// The folders of answers the tests read: this client's recorded answers, the
/// demo's, the protocol's corpus and the core's transcripts.
private let answerFolders = [
    "packages/apple/Tests/NavreeDesignTests/Answers",
    "packages/apple/Demo/NavreeDemo/Recorded",
    "crates/navree-native/corpus",
    "crates/navree-native-core/transcripts/fixtures",
]

/// Every icon in an answer: its name, and whether the server named its
/// symbol itself.
private func icons(in node: Any, key: String?, into found: inout [String: Bool]) {
    if let object = node as? [String: Any] {
        let isIcon = key == "icon" || object["kind"] as? String == "icon" || object["t"] as? String == "icon"
        if isIcon, let name = object["name"] as? String {
            let named = ((object["symbol"] as? [String: Any])?["apple"] as? String).map { !$0.isEmpty } ?? false
            found[name] = (found[name] ?? false) || named
        }
        if key == "icons" {
            for name in object.keys where found[name] == nil { found[name] = false }
        }
        for (child, value) in object { icons(in: value, key: child, into: &found) }
    } else if let array = node as? [Any] {
        for value in array { icons(in: value, key: key, into: &found) }
    }
}

private func answerIcons() throws -> [String: Bool] {
    var found: [String: Bool] = [:]
    var files = 0
    for folder in answerFolders {
        let base = root.appendingPathComponent(folder)
        let walker = try #require(FileManager.default.enumerator(at: base, includingPropertiesForKeys: nil))
        for case let url as URL in walker where url.pathExtension == "json" {
            let data = try Data(contentsOf: url)
            guard let json = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]) else { continue }
            files += 1
            icons(in: json, key: nil, into: &found)
        }
    }
    #expect(files > 100, "the answers were read")
    return found
}

/// The toolkit's icon names, current and old, from its generated set.
private func toolkitNames() throws -> Set<String> {
    let source = try String(contentsOf: root.appendingPathComponent("crates/navree-ui/src/icons/lucide.rs"), encoding: .utf8)
    let pairs = try Regex(#"\("([a-z0-9-]+)", "?[A-Za-z0-9_-]+"?\)"#)
    var names = Set<String>()
    for match in source.matches(of: pairs) {
        if let name = match.output[1].substring { names.insert(String(name)) }
    }
    return names
}

@Test func everyIconOfTheAnswersTheCorpusAndTheTranscriptsHasASymbol() throws {
    let found = try answerIcons()
    #expect(found.count > 40)
    let drawn = found.filter { name, named in !named && Symbols.system(for: name) == nil }.keys.sorted()
    #expect(drawn.isEmpty, "drawn from their paths: \(drawn)")
}

@Test func theClientsTableHoldsNoNameOfTheProtocols() {
    let both = Set(AppleSymbols.names.keys).intersection(ProtocolSymbols.names.keys)
    #expect(both.isEmpty, "\(both.sorted())")
}

@Test func everyNameOfTheClientsTableIsOneOfTheToolkitsIcons() throws {
    let names = try toolkitNames()
    #expect(names.count > 2_000)
    let unknown = AppleSymbols.names.keys.filter { !names.contains($0) }.sorted()
    #expect(unknown.isEmpty, "\(unknown)")
}

@Test func everySymbolOfTheClientsTableLoadsOnThisSystem() {
    for (name, entry) in AppleSymbols.names {
        for symbol in [entry.apple] + (entry.filled.map { [$0] } ?? []) {
            #if canImport(AppKit)
            let image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            #else
            let image = UIImage(systemName: symbol)
            #endif
            #expect(image != nil, "\(name): \(symbol)")
        }
        if let value = entry.value { #expect((0...1).contains(value), "\(name)") }
    }
}

/// The system's list of its symbols and the release each came in, where this
/// machine has it (a Mac's CoreGlyphs).
private let availability = URL(fileURLWithPath: "/System/Library/CoreServices/CoreGlyphs.bundle/Contents/Resources/name_availability.plist")

@Test(.enabled(if: FileManager.default.fileExists(atPath: availability.path)))
func everySymbolOfBothTablesIsOnIOS26AndMacOS26() throws {
    let plist = try #require(
        try PropertyListSerialization.propertyList(from: Data(contentsOf: availability), format: nil) as? [String: Any]
    )
    let symbols = try #require(plist["symbols"] as? [String: String])
    let releases = try #require(plist["year_to_release"] as? [String: [String: String]])
    func atMost26(_ version: String?) -> Bool {
        guard let major = version?.split(separator: ".").first.flatMap({ Int($0) }) else { return false }
        let minor = version?.split(separator: ".").dropFirst().first.flatMap { Int($0) } ?? 0
        return major < 26 || (major == 26 && minor == 0)
    }
    var all: [(String, String)] = []
    for (name, entry) in AppleSymbols.names {
        all.append((name, entry.apple))
        if let filled = entry.filled { all.append((name, filled)) }
    }
    for (name, pair) in ProtocolSymbols.names {
        all.append((name, pair.apple))
        if let filled = pair.filled { all.append((name, filled)) }
    }
    for (name, symbol) in all {
        let year = symbols[symbol]
        #expect(year != nil, "\(name): \(symbol) is not one of the system's symbols")
        guard let year else { continue }
        let release = releases[year]
        #expect(atMost26(release?["iOS"]) && atMost26(release?["macOS"]), "\(name): \(symbol) came in \(year)")
    }
}

@Test func anIconTakesTheServersSymbolThenTheProtocolsThenTheClientsThenItsShape() throws {
    let protocolName = try decode(Navree.Icon.self, #"{"name":"inbox"}"#)
    let clientName = try decode(Navree.Icon.self, #"{"name":"indian-rupee"}"#)
    let given = try decode(Navree.Icon.self, #"{"name":"indian-rupee","symbol":{"apple":"banknote","material":"payments"}}"#)
    let level = try decode(Navree.Icon.self, #"{"name":"signal-high"}"#)
    let unknown = try decode(Navree.Icon.self, #"{"name":"ribbon","paths":["M12 11.22C11 9.997 10 9 10 8a2 2 0 0 1 4 0c0 1-.998 2.002-2.01 3.22"]}"#)
    #expect(Symbols.symbol(for: protocolName) == "tray")
    #expect(Symbols.filled(for: protocolName) == "tray.fill")
    #expect(Symbols.symbol(for: clientName) == "indianrupeesign")
    #expect(Symbols.filled(for: clientName) == "indianrupeesign")
    #expect(Symbols.symbol(for: given) == "banknote")
    #expect(Symbols.level(for: given) == nil)
    #expect(Symbols.symbol(for: level) == "cellularbars")
    #expect(Symbols.level(for: level) == 0.75)
    #expect(Symbols.symbol(for: unknown) == nil)
    #expect(Symbols.image(for: unknown) == nil)
    #expect(Symbols.system(for: "package-plus") == "shippingbox")
    #expect(Symbols.system(for: "boxes") == "cube.box")
}
