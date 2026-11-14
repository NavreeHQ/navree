import Foundation
import Security
import WebKit
import os

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
        if secure && url.scheme?.lowercased() != "https" && !Self.isLoopback(host) { return false }
        guard matches(host: host) else { return false }
        let requestPath = url.path.isEmpty ? "/" : url.path
        return requestPath == path || requestPath.hasPrefix(path.hasSuffix("/") ? path : path + "/")
    }

    /// Whether it belongs to `host`, as a browser matches a cookie's domain.
    func matches(host: String) -> Bool {
        let domain = self.domain.lowercased()
        return domain.hasPrefix(".")
            ? host == domain.dropFirst() || host.hasSuffix(domain)
            : host == domain
    }

    /// This machine, where a browser treats plain `http` as secure.
    static func isLoopback(_ host: String) -> Bool {
        host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]"
    }
}

/// The one place the session lives, by name: the native requests read and
/// write it, it is copied into the web view's store before a page loads, and
/// it takes back what a page set. Its master copy is in the Keychain, for this
/// device only, so a backup restored elsewhere is not signed in.
///
/// It holds what the origin's answers set: the runtime core decides which
/// addresses are asked, and the jar keeps what they say.
@MainActor
public final class CookieJar {
    private(set) var records: [CookieRecord] = []
    private let keychain: KeychainSlot?
    /// The origin whose session this is: what every address a request, a
    /// picture or the web view goes to is checked against. `nil` when the
    /// configured text is not an origin, and then no address is ours.
    public let site: Origin?

    /// A jar for the app at `origin`; `persistent` keeps it in the Keychain.
    public init(origin: String, loopbackHTTP: Bool = false, persistent: Bool = true) {
        site = Origin(origin, loopbackHTTP: loopbackHTTP)
        keychain = persistent ? KeychainSlot(service: "com.navree.native.session", account: origin) : nil
        if let data = keychain?.read(), let saved = try? JSONDecoder().decode([CookieRecord].self, from: data) {
            records = saved.filter { $0.expires.map { $0 > Date() } ?? true }
        }
    }

    /// The `Cookie` header for a request to `url`, or `nil` when no cookie goes.
    public func header(for url: URL, at now: Date = Date()) -> String? {
        let matching = records.filter { $0.applies(to: url, at: now) }
            .sorted { $0.path.count > $1.path.count }
        return matching.isEmpty ? nil : matching.map { "\($0.name)=\($0.value)" }.joined(separator: "; ")
    }

    /// The value an `x-xsrf-token` header carries for `url`: the `XSRF-TOKEN`
    /// cookie, percent-decoded. `nil` when there is none.
    public func xsrfToken(for url: URL, at now: Date = Date()) -> String? {
        let cookie = records.filter { $0.name == "XSRF-TOKEN" && $0.applies(to: url, at: now) }
            .max { $0.path.count < $1.path.count }
        return cookie.map { $0.value.removingPercentEncoding ?? $0.value }
    }

    /// Takes the `Set-Cookie` headers of an answer, a redirect's too.
    public func take(from response: HTTPURLResponse, for url: URL) {
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

    /// Takes back what a page of `host` set in the web view's store, when
    /// `host` is the origin's own: never another site's.
    public func adopt(from store: WKHTTPCookieStore, host: String) async {
        guard let site, host.lowercased() == site.host else { return }
        let cookies = await store.allCookies().map(CookieRecord.init).filter { $0.matches(host: host.lowercased()) }
        let others = records.filter { !$0.matches(host: host.lowercased()) }
        let next = others + cookies
        guard next != records else { return }
        records = next
        save()
    }

    /// The hosts the jar holds cookies for: the app's origins.
    public var hosts: Set<String> {
        Set(records.map { $0.domain.hasPrefix(".") ? String($0.domain.dropFirst()) : $0.domain }.map { $0.lowercased() })
    }

    /// Ends the session on this device: the jar and its Keychain item are
    /// emptied, and the web view's store loses every cookie and all stored
    /// data of the same hosts.
    public func forget(_ store: WKWebsiteDataStore?) async {
        let hosts = self.hosts
        records = []
        keychain?.delete()
        guard let store else { return }
        for cookie in await store.httpCookieStore.allCookies() {
            let record = CookieRecord(cookie)
            if hosts.contains(where: { record.matches(host: $0) }) {
                await store.httpCookieStore.deleteCookie(cookie)
            }
        }
        let kinds = WKWebsiteDataStore.allWebsiteDataTypes()
        let found = await store.dataRecords(ofTypes: kinds)
        let ours = found.filter { record in
            hosts.contains { host in record.displayName == host || host.hasSuffix("." + record.displayName) }
        }
        await store.removeData(ofTypes: kinds, for: ours)
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
        var status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var item = query
            item.merge(attributes) { _, new in new }
            status = SecItemAdd(item as CFDictionary, nil)
        }
        // A build with no signature at all has no Keychain: the session then
        // lasts only as long as the app runs.
        if status != errSecSuccess {
            Self.log.error("the session could not be kept in the Keychain: \(status, privacy: .public)")
        }
    }

    static let log = Logger(subsystem: "com.navree.native", category: "session")

    func delete() {
        SecItemDelete(query as CFDictionary)
    }
}
