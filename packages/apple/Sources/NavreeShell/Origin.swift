import Foundation

/// The app's one origin, as the build configures it, and the one question
/// asked of an address before the app fetches it, streams from it, loads it
/// in the web view or gives it the session: is this whole address ours?
///
/// An address is read once, by the system's own parser (`URLComponents`),
/// and what it read is what is used: the `URL` given back is written from the
/// parts read, never taken from the text. Text with white space, a control
/// character or a backslash anywhere is refused, never trimmed.
public struct Origin: Sendable, Equatable {
    /// `https`, or `http` for a server on this machine.
    public let scheme: String
    /// The host in lower case; an IPv6 literal keeps its brackets.
    public let host: String
    /// The port as configured, or `nil` for the scheme's own.
    public let port: Int?

    /// The origin `text` names (`https://shop.example`, a trailing `/`
    /// allowed), or `nil` when it is not one. Plain `http` is an origin only
    /// with `loopbackHTTP` and a host on this machine.
    public init?(_ text: String, loopbackHTTP: Bool = false) {
        guard let read = Self.read(text), read.path.isEmpty || read.path == "/", read.query == nil, read.fragment == nil
        else { return nil }
        guard read.scheme == "https" || (loopbackHTTP && Self.loopback.contains(read.host)) else { return nil }
        scheme = read.scheme
        host = read.host
        port = read.port
    }

    /// The whole address `text` as a `URL` when it is ours: `http` or `https`
    /// with no user and no password, this origin's scheme, host and port, and
    /// a path that does not begin with `//`. Anything else is `nil`.
    public func inside(_ text: String) -> URL? {
        guard let read = Self.read(text), read.scheme == scheme, read.host == host,
            (read.port ?? Self.defaultPort(read.scheme)) == (port ?? Self.defaultPort(scheme)),
            !read.path.hasPrefix("//")
        else { return nil }
        return read.url
    }

    /// An address on this origin that an answer names: a path that begins
    /// with one `/`, joined to the origin, or a whole address that is ours.
    /// Never read against the origin as a relative address is.
    public func address(_ text: String) -> URL? {
        if text.hasPrefix("/") {
            return text.hasPrefix("//") ? nil : inside(self.text + text)
        }
        return inside(text)
    }

    /// The address of a picture the server names: a path that begins with
    /// one `/`, on this origin; a whole address that is ours; or a whole
    /// `https` address on the scheme's own port whose host is one of `hosts`
    /// exactly. Anything else is `nil`, and the picture's place stays empty.
    public func picture(_ source: String, hosts: [String]) -> URL? {
        if let url = address(source) { return url }
        guard let read = Self.read(source), read.scheme == "https", read.port == nil || read.port == 443,
            hosts.contains(where: { $0.lowercased() == read.host }), !read.path.hasPrefix("//")
        else { return nil }
        return read.url
    }

    /// The origin as text, `scheme://host[:port]`, with no path.
    public var text: String {
        scheme + "://" + host + (port.map { ":\($0)" } ?? "")
    }

    /// Hosts on this machine, where plain `http` may be allowed in development.
    static let loopback: Set<String> = ["localhost", "127.0.0.1", "[::1]"]

    static func defaultPort(_ scheme: String) -> Int {
        scheme == "http" ? 80 : 443
    }

    // MARK: - Reading

    /// What `URLComponents` read of a whole `http` or `https` address.
    struct Read: Equatable {
        /// Lower case.
        var scheme: String
        /// Lower case.
        var host: String
        var port: Int?
        /// The path, query and fragment as read, percent-encoded.
        var path: String
        var query: String?
        var fragment: String?

        /// The address written again from its parts.
        var url: URL? {
            var text = scheme + "://" + host
            if let port { text += ":\(port)" }
            text += path
            if let query { text += "?" + query }
            if let fragment { text += "#" + fragment }
            return URL(string: text)
        }
    }

    /// Reads `text` as a whole `http` or `https` address with a host and no
    /// user information, or `nil`.
    static func read(_ text: String) -> Read? {
        guard isClean(text), let parts = URLComponents(string: text, encodingInvalidCharacters: true),
            let scheme = parts.scheme?.lowercased(), scheme == "https" || scheme == "http",
            parts.percentEncodedUser == nil, parts.percentEncodedPassword == nil,
            let encoded = parts.encodedHost, let host = hostName(encoded)
        else { return nil }
        if let port = parts.port, !(1...65535).contains(port) { return nil }
        return Read(
            scheme: scheme, host: host, port: parts.port, path: parts.percentEncodedPath,
            query: parts.percentEncodedQuery, fragment: parts.percentEncodedFragment
        )
    }

    /// Whether `text` holds no white space, no control character and no
    /// backslash anywhere.
    static func isClean(_ text: String) -> Bool {
        !text.isEmpty
            && !text.unicodeScalars.contains { scalar in
                scalar == "\\" || scalar.properties.isWhitespace || scalar.properties.generalCategory == .control
                    || scalar.properties.generalCategory == .format
            }
    }

    /// The host in lower case when it is a name of ASCII letters, digits, `-`
    /// and `.`, or an IPv6 literal in brackets; `nil` for anything else.
    static func hostName(_ encoded: String) -> String? {
        let host = encoded.lowercased()
        guard !host.isEmpty else { return nil }
        if host.utf8.allSatisfy(isNameByte) { return host }
        let inner = host.utf8.dropFirst().dropLast()
        let literal = host.hasPrefix("[") && host.hasSuffix("]") && !inner.isEmpty && inner.allSatisfy(isLiteralByte)
        return literal ? host : nil
    }

    static func isNameByte(_ byte: UInt8) -> Bool {
        switch byte {
        case 0x61...0x7A, 0x30...0x39, 0x2D, 0x2E: true
        default: false
        }
    }

    static func isLiteralByte(_ byte: UInt8) -> Bool {
        switch byte {
        case 0x61...0x66, 0x30...0x39, 0x3A, 0x2E: true
        default: false
        }
    }
}
