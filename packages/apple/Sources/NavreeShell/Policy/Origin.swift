import Foundation

/// The one origin the app talks to: its native requests, its web view and its
/// deep links. It is compiled into the app, never taken from a server.
///
/// A pattern may name one level of subdomains, for apps whose workspaces live
/// on their own hosts: `https://*.example.com` allows `https://acme.example.com`
/// and not `https://example.com` or `https://a.b.example.com`. Plain `http` is
/// allowed only for this machine (`localhost`, `127.0.0.1`, `::1`), for
/// development against a local server.
public struct Origin: Sendable, Equatable, CustomStringConvertible {
    public let scheme: String
    /// The host, lowercased; it starts with `*.` for a pattern.
    public let host: String
    public let port: Int?

    /// Reads `https://store.example.com`, `https://*.example.com` or
    /// `http://127.0.0.1:8095`. Anything with a path, a query, userinfo, or
    /// plain `http` to another machine is refused.
    public init?(_ text: String) {
        guard let components = URLComponents(string: text.replacingOccurrences(of: "://*.", with: "://wildcard-.")),
            let scheme = components.scheme?.lowercased(),
            var host = components.host?.lowercased(), !host.isEmpty,
            components.user == nil, components.password == nil,
            components.path.isEmpty || components.path == "/",
            components.query == nil, components.fragment == nil
        else { return nil }
        if host.hasPrefix("wildcard-.") {
            host = "*." + host.dropFirst("wildcard-.".count)
        }
        guard scheme == "https" || (scheme == "http" && Self.isLoopback(host)) else { return nil }
        self.scheme = scheme
        self.host = host
        self.port = components.port
    }

    /// Whether the origin is a pattern rather than one host.
    public var isPattern: Bool {
        host.hasPrefix("*.")
    }

    /// The origin as a URL, for a fixed host: what relative addresses are read against.
    public var url: URL? {
        guard !isPattern else { return nil }
        var components = URLComponents()
        components.scheme = scheme
        components.host = host
        components.port = port
        components.path = "/"
        return components.url
    }

    public var description: String {
        "\(scheme)://\(host)" + (port.map { ":\($0)" } ?? "")
    }

    /// Whether `url` is on this origin: scheme, host and port all match, and it
    /// carries no userinfo.
    public func contains(_ url: URL) -> Bool {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: true),
            components.scheme?.lowercased() == scheme,
            let host = components.host?.lowercased(),
            components.user == nil, components.password == nil,
            effectivePort(components.port) == effectivePort(port)
        else { return false }
        if isPattern {
            let suffix = String(self.host.dropFirst(1))  // ".example.com"
            guard host.hasSuffix(suffix) else { return false }
            let label = host.dropLast(suffix.count)
            return !label.isEmpty && !label.contains(".")
        }
        return host == self.host
    }

    private func effectivePort(_ port: Int?) -> Int {
        port ?? (scheme == "https" ? 443 : 80)
    }

    static func isLoopback(_ host: String) -> Bool {
        host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]"
    }
}

/// Where an address leads, as the app sees it.
public enum Destination: Sendable, Equatable {
    /// A screen of the app: its path and query, such as `/orders?sort=total`.
    case app(String)
    /// Another site or another app (mail, phone): the system opens it.
    case external(URL)
    /// Nothing the app will open: a script, a file, data, an unknown scheme.
    case refused
}

extension Origin {
    /// Where `address` leads when it appears on a page of the app, read against
    /// `page` when it is relative.
    public func destination(of address: String, from page: URL? = nil) -> Destination {
        if let absolute = URL(string: address), absolute.scheme != nil {
            return destination(of: absolute)
        }
        guard let url = URL(string: address, relativeTo: page ?? self.url)?.absoluteURL else { return .refused }
        return destination(of: url)
    }

    /// Where `url` leads.
    public func destination(of url: URL) -> Destination {
        let scheme = url.scheme?.lowercased() ?? ""
        if contains(url) {
            return .app(Self.pathAndQuery(of: url))
        }
        switch scheme {
        case "https", "http":
            guard let components = URLComponents(url: url, resolvingAgainstBaseURL: true),
                components.user == nil, components.password == nil, components.host != nil
            else { return .refused }
            return .external(url)
        case "mailto", "tel", "sms", "facetime", "maps":
            return .external(url)
        default:
            return .refused
        }
    }

    /// `/orders?sort=total` from a URL on the origin; `/` for an empty path.
    public static func pathAndQuery(of url: URL) -> String {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: true) else { return "/" }
        let path = components.percentEncodedPath.isEmpty ? "/" : components.percentEncodedPath
        if let query = components.percentEncodedQuery, !query.isEmpty {
            return path + "?" + query
        }
        return path
    }
}
