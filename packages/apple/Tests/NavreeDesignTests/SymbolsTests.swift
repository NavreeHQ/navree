import Foundation
import NavreeDesign
import NavreeProtocol
import Testing

// The system symbols come from the protocol's own table, which this client
// keeps as the schema has it; its own table is read after it.

/// The protocol's schema, kept in the repository.
private func schemaSymbols() throws -> [String: [String: [String: String]]] {
    let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let data = try Data(contentsOf: root.appendingPathComponent("crates/navree-native/schema/navree-native.schema.json"))
    let schema = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    return try #require(schema["x-symbols"] as? [String: [String: [String: String]]])
}

@Test func theSymbolTableIsTheProtocolsOwn() throws {
    let symbols = try schemaSymbols()
    let names = try #require(symbols["names"])
    #expect(Set(ProtocolSymbols.names.keys) == Set(names.keys))
    for (name, entry) in names {
        #expect(ProtocolSymbols.names[name] == ProtocolSymbols.Pair(entry["apple"] ?? "", entry["apple_filled"]), "\(name)")
    }
    let standard = try #require(symbols["standard"])
    #expect(ProtocolSymbols.standard.count == standard.count)
    for action in Navree.Standard.allCases {
        let entry = try #require(standard[action.rawValue])
        #expect(ProtocolSymbols.standard[action] == ProtocolSymbols.Pair(entry["apple"] ?? "", entry["apple_filled"]))
    }
}

@Test func anIconTakesTheServersSymbolThenTheTablesThenItsOwnShape() throws {
    let named = try decode(Navree.Icon.self, #"{"name":"send"}"#)
    let given = try decode(Navree.Icon.self, #"{"name":"send","symbol":{"apple":"paperplane.circle","material":"send"}}"#)
    let unknown = try decode(Navree.Icon.self, #"{"name":"candy","paths":["M10 7v10.9"]}"#)
    #expect(Symbols.symbol(for: named) == "paperplane")
    #expect(Symbols.filled(for: named) == "paperplane.fill")
    #expect(Symbols.symbol(for: given) == "paperplane.circle")
    #expect(Symbols.symbol(for: unknown) == nil)
    #expect(Symbols.standard(.share) == "square.and.arrow.up")
}
