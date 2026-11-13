import Foundation
import NavreeProtocol

/// The one check every way into the app goes through: a universal link, the
/// app's own scheme, a notification's address, a widget's tap.
///
/// A link only ever opens a screen with a plain GET. It never posts a form,
/// starts a device call or confirms anything; the screen that opens may offer
/// the action, and the person taps it.
public struct DeepLinks: Sendable {
    public let origin: Origin
    /// The app's own URL scheme (`charkha`), when it has one. Links in it are
    /// not verified by the system, so they open only paths the shell names.
    public let scheme: String?

    public init(origin: Origin, scheme: String? = nil) {
        self.origin = origin
        self.scheme = scheme?.lowercased()
    }

    /// The path and query to open for `url`, or `nil` when the link is refused
    /// (the app then shows its home). A path opens when the shell's routes or
    /// its destinations name it.
    public func address(for url: URL, shell: Navree.Shell?) -> String? {
        let candidate: String
        if origin.contains(url) {
            candidate = Origin.pathAndQuery(of: url)
        } else if let scheme, url.scheme?.lowercased() == scheme {
            // `charkha://orders/1?tab=items`: the host is the first segment.
            guard var components = URLComponents(url: url, resolvingAgainstBaseURL: false),
                components.user == nil, components.password == nil
            else { return nil }
            let host = components.host ?? ""
            components.path = "/" + ([host] + components.path.split(separator: "/").map(String.init))
                .filter { !$0.isEmpty }.joined(separator: "/")
            candidate = components.percentEncodedPath + (components.percentEncodedQuery.map { "?" + $0 } ?? "")
        } else {
            return nil
        }
        guard let address = Self.normalised(candidate) else { return nil }
        guard let shell, Self.isNamed(address, by: shell) else { return nil }
        return address
    }

    /// The path made safe: decoded once, with no `..`, backslash or control
    /// character, and repeated slashes folded. The query is kept as it came.
    static func normalised(_ address: String) -> String? {
        let parts = address.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false)
        guard let rawPath = parts.first, let path = String(rawPath).removingPercentEncoding else { return nil }
        if path.contains("\\") || path.unicodeScalars.contains(where: { $0.value < 0x20 || $0.value == 0x7F }) {
            return nil
        }
        let segments = path.split(separator: "/", omittingEmptySubsequences: true)
        if segments.contains(where: { $0 == ".." || $0 == "." }) {
            return nil
        }
        let clean = "/" + segments.joined(separator: "/")
        guard let encoded = clean.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) else { return nil }
        return parts.count > 1 ? encoded + "?" + parts[1] : encoded
    }

    /// Whether the shell names `address`: a route's pattern or a destination.
    static func isNamed(_ address: String, by shell: Navree.Shell) -> Bool {
        let path = String(address.split(separator: "?", maxSplits: 1).first ?? "")
        if RouteRules(shell.routes).rule(for: path) != nil {
            return true
        }
        let destinations = shell.destinations.flatMap { [$0] + $0.items }.map(\.href)
        return destinations.contains(path) || path == shell.home
    }
}
