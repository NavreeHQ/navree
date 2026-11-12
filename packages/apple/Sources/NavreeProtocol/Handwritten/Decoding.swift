import Foundation

// The protocol's types are written by hand in this folder until they are
// generated from the protocol crate. Every field a server may leave out holds
// its default, a field the client does not know is passed over, and a kind or
// a value the client does not know decodes to a fallback instead of failing:
// an app in a store is often older than the server it talks to.

/// A coding key for any name, so each type reads exactly the fields it knows.
public struct WireKey: CodingKey, Sendable {
    public let stringValue: String
    public var intValue: Int? { nil }

    public init(_ name: String) {
        stringValue = name
    }

    public init?(stringValue: String) {
        self.stringValue = stringValue
    }

    public init?(intValue: Int) {
        return nil
    }
}

extension KeyedDecodingContainer where Key == WireKey {
    /// A field that is always there.
    func required<T: Decodable>(_ name: String) throws -> T {
        try decode(T.self, forKey: WireKey(name))
    }

    /// A field that may be left out, read as `nil` then.
    func optional<T: Decodable>(_ name: String) throws -> T? {
        try decodeIfPresent(T.self, forKey: WireKey(name))
    }

    /// A field that may be left out, read as `fallback` then.
    func field<T: Decodable>(_ name: String, or fallback: T) throws -> T {
        try decodeIfPresent(T.self, forKey: WireKey(name)) ?? fallback
    }

    /// Whether the object has this field at all.
    func has(_ name: String) -> Bool {
        contains(WireKey(name))
    }
}

extension Decoder {
    /// A field-less object's keyed reader.
    func wire() throws -> KeyedDecodingContainer<WireKey> {
        try container(keyedBy: WireKey.self)
    }

    /// One of a closed set of names, or `fallback` for a name added after this
    /// client was built.
    func closedSet<E: RawRepresentable>(or fallback: E) throws -> E where E.RawValue == String {
        let raw = try singleValueContainer().decode(String.self)
        return E(rawValue: raw) ?? fallback
    }
}

/// Any JSON value: what a plugin's view is given.
public enum JSONValue: Sendable, Equatable, Decodable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: any Decoder) throws {
        let single = try decoder.singleValueContainer()
        if single.decodeNil() {
            self = .null
        } else if let value = try? single.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? single.decode(Double.self) {
            self = .number(value)
        } else if let value = try? single.decode(String.self) {
            self = .string(value)
        } else if let value = try? single.decode([JSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try single.decode([String: JSONValue].self))
        }
    }
}

/// Reads protocol JSON.
public enum WireDecoder {
    /// Decodes an envelope, a shell document or any other protocol type.
    public static func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        try JSONDecoder().decode(type, from: data)
    }
}
