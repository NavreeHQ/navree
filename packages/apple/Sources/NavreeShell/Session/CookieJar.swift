import Foundation
import Security
import WebKit

/// A cookie of the app's origin, as the jar keeps it.
public struct CookieRecord: Sendable, Equatable, Codable {
    public var name: String
    public var value: String
    public var domain: String
    public var path: String
    public var expires: Date?
    public var secure: Bool
    public var httpOnly: Bool
    public var sameSite: String?

    public init(
        name: String, value: String, domain: String, path: String = "/", expires: Date? = nil,
        secure: Bool = true, httpOnly: Bool = true, sameSite: String? = "lax"
    ) {
        self.name = name
        self.value = value
        self.domain = domain
        self.path = path
        self.expires = expires
        self.secure = secure
        self.httpOnly = httpOnly
        self.sameSite = sameSite
    }

    public init(_ cookie: HTTPCookie) {
        name = cookie.name
        value = cookie.value
        domain = cookie.domain
        path = cookie.path
        expires = cookie.expiresDate
        secure = cookie.isSecure
        httpOnly = cookie.isHTTPOnly
        sameSite = cookie.sameSitePolicy?.rawValue
    }

    /// The system's cookie, to give the web view.
    public var cookie: HTTPCookie? {
        var properties: [HTTPCookiePropertyKey: Any] = [
            .name: name, .value: value, .domain: domain, .path: path,
        ]
        if let expires { properties[.expires] = expires }
        if secure { properties[.secure] = "TRUE" }
        if httpOnly { properties[HTTPCookiePropertyKey("HttpOnly")] = "TRUE" }
        if let sameSite { properties[.sameSitePolicy] = sameSite }
        return HTTPCookie(properties: properties)
    }

    /// Whether it goes with a request to `url` at `now`.
    func applies(to url: URL, at now: Date) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        if let expires, expires <= now { return false }
        if secure && url.scheme?.lowercased() != "https" && !Origin.isLoopback(host) { return false }
        let domain = self.domain.lowercased()
        let hostMatches = domain.hasPrefix(".")
            ? host == domain.dropFirst() || host.hasSuffix(domain)
            : host == domain
        guard hostMatches else { return false }
        let requestPath = url.path.isEmpty ? "/" : url.path
        return requestPath == path || requestPath.hasPrefix(path.hasSuffix("/") ? path : path + "/")
    }
}

/// The one place the session lives: the native requests read and write it,
/// and it is copied into the web view's store before a page loads and read
/// back from it when a page signs in or out. Its master copy is in the
/// Keychain, for this device only, so a backup restored elsewhere is not
/// signed in.
@MainActor
public final class CookieJar {
    public let origin: Origin
    private(set) var records: [CookieRecord] = []
    private let keychain: KeychainSlot?

    /// A jar for `origin`; `persistent` keeps it in the Keychain.
    public init(origin: Origin, persistent: Bool = true) {
        self.origin = origin
        keychain = persistent ? KeychainSlot(service: "com.navree.native.session", account: origin.description) : nil
        if let data = keychain?.read(), let saved = try? JSONDecoder().decode([CookieRecord].self, from: data) {
            records = saved.filter { $0.expires.map { $0 > Date() } ?? true }
        }
    }

    /// The `Cookie` header for a request to `url`, or `nil` when no cookie goes.
    public func header(for url: URL, at now: Date = Date()) -> String? {
        guard origin.contains(url) else { return nil }
        let matching = records.filter { $0.applies(to: url, at: now) }
            .sorted { $0.path.count > $1.path.count }
        return matching.isEmpty ? nil : matching.map { "\($0.name)=\($0.value)" }.joined(separator: "; ")
    }

    /// Takes the `Set-Cookie` headers of an answer from the origin.
    public func take(from response: HTTPURLResponse, for url: URL) {
        guard origin.contains(url) else { return }
        var fields: [String: String] = [:]
        for (key, value) in response.allHeaderFields {
            if let key = key as? String, let value = value as? String { fields[key] = value }
        }
        let cookies = HTTPCookie.cookies(withResponseHeaderFields: fields, for: url).map(CookieRecord.init)
        guard !cookies.isEmpty else { return }
        merge(cookies)
    }

    /// Puts `cookies` in, replacing those of the same name, domain and path,
    /// and dropping any that have expired.
    public func merge(_ cookies: [CookieRecord], at now: Date = Date()) {
        for cookie in cookies {
            records.removeAll { $0.name == cookie.name && $0.domain == cookie.domain && $0.path == cookie.path }
            if cookie.expires.map({ $0 > now }) ?? true {
                records.append(cookie)
            }
        }
        save()
    }

    /// Copies the session into the web view's store, before a page loads.
    public func copy(into store: WKHTTPCookieStore) async {
        for record in records {
            if let cookie = record.cookie {
                await store.setCookie(cookie)
            }
        }
    }

    /// Reads back what a page in the web view set for the origin.
    public func adopt(from store: WKHTTPCookieStore) async {
        let cookies = await store.allCookies()
        let ours = cookies.filter { cookie in
            var components = URLComponents()
            components.scheme = origin.scheme
            components.host = cookie.domain.hasPrefix(".") ? String(cookie.domain.dropFirst()) : cookie.domain
            return components.url.map(origin.contains) ?? false
        }
        let records = ours.map(CookieRecord.init)
        guard records != self.records else { return }
        self.records = records
        save()
    }

    /// Ends the session on this device: the jar, the Keychain and the web
    /// view's store are emptied.
    public func clear(_ store: WKHTTPCookieStore?) async {
        let removed = records.compactMap(\.cookie)
        records = []
        keychain?.delete()
        if let store {
            for cookie in removed {
                await store.deleteCookie(cookie)
            }
        }
    }

    private func save() {
        guard let keychain else { return }
        if records.isEmpty {
            keychain.delete()
        } else if let data = try? JSONEncoder().encode(records) {
            keychain.write(data)
        }
    }
}

/// One Keychain item that never syncs and never moves to another device, and
/// that can be read after the first unlock (so a refresh in the background works).
struct KeychainSlot: Sendable {
    let service: String
    let account: String

    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account]
    }

    func read() -> Data? {
        var query = self.query
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return nil }
        return result as? Data
    }

    func write(_ data: Data) {
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        if SecItemUpdate(query as CFDictionary, attributes as CFDictionary) == errSecItemNotFound {
            var item = query
            item.merge(attributes) { _, new in new }
            SecItemAdd(item as CFDictionary, nil)
        }
    }

    func delete() {
        SecItemDelete(query as CFDictionary)
    }
}
