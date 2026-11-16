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

    /// What tells one cookie from another: its name, its domain and its path.
    struct Key: Hashable, Sendable {
        var name: String
        var domain: String
        var path: String
    }

    var key: Key {
        Key(name: name, domain: domain.lowercased(), path: path)
    }

    /// This machine, where a browser treats plain `http` as secure.
    static func isLoopback(_ host: String) -> Bool {
        host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]"
    }
}

/// The cookies of a web view's store, as the jar reads and writes them: the
/// system's `WKHTTPCookieStore`, or a stand-in in tests.
@MainActor
protocol WebCookies: AnyObject {
    /// Every cookie the store holds.
    func held() async -> [HTTPCookie]
    func put(_ cookie: HTTPCookie) async
    func drop(_ cookie: HTTPCookie) async
}

extension WKHTTPCookieStore: WebCookies {
    func held() async -> [HTTPCookie] { await allCookies() }
    func put(_ cookie: HTTPCookie) async { await setCookie(cookie) }
    func drop(_ cookie: HTTPCookie) async { await deleteCookie(cookie) }
}

/// The one place the session's cookies live, for every window of the app:
/// the native requests read and write it, the web view's store is made to
/// hold the same before a page loads, and it takes what a page set. Its
/// master copy is one Keychain item (``KeychainSlot`` says what the system
/// does with it), so the web view's store is never where a session comes
/// back from.
///
/// It holds what the origin's answers set: the runtime core decides which
/// addresses are asked, and the jar keeps what they say.
@MainActor
public final class CookieJar {
    private(set) var records: [CookieRecord] = []
    private let slot: (any SecretSlot)?
    /// The origin whose session this is: what every address a request, a
    /// picture or the web view goes to is checked against. `nil` when the
    /// configured text is not an origin, and then no address is ours.
    public let site: Origin?

    /// The Keychain service every app's session is kept under, by its origin.
    static let service = "com.navree.native.session"

    /// A jar for the app at `origin`; `persistent` keeps it in the Keychain.
    public convenience init(origin: String, loopbackHTTP: Bool = false, persistent: Bool = true) {
        self.init(
            origin: origin, loopbackHTTP: loopbackHTTP,
            slot: persistent ? KeychainSlot(service: Self.service, account: origin) : nil
        )
    }

    /// A jar whose master copy is kept in `slot`. With `reads` false nothing
    /// is taken from the slot: the session it holds was ended.
    init(origin: String, loopbackHTTP: Bool = false, slot: (any SecretSlot)?, reads: Bool = true) {
        site = Origin(origin, loopbackHTTP: loopbackHTTP)
        self.slot = slot
        if reads, let data = slot?.read(), let saved = try? JSONDecoder().decode([CookieRecord].self, from: data) {
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

    /// Takes the `Set-Cookie` headers of an answer, a redirect's too. With
    /// `session`, the number the jar had when the request went out: an answer
    /// that belongs to a session that has since ended is dropped.
    public func take(from response: HTTPURLResponse, for url: URL, session: Int? = nil) {
        if let session, session != self.session { return }
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
            records.removeAll { $0.key == cookie.key }
            if cookie.expires.map({ $0 > now }) ?? true {
                records.append(cookie)
            }
        }
        save()
    }

    // MARK: - The web view's store

    /// What the web view's store held of the origin's cookies, by value, when
    /// the jar last looked: what tells a cookie a page set, changed or took
    /// away from one that only stayed. `nil` until the jar has made the store
    /// of this session its own.
    private var seen: [CookieRecord.Key: String]?
    /// Whether a piece of work on the web view's store is under way, and what waits for it to end.
    private var working = false
    private var waiting: [CheckedContinuation<Void, Never>] = []

    /// Before a page loads: takes what the page before it set (as
    /// ``adopt(from:host:session:)`` does), then makes the store hold the
    /// session's cookies for the origin and no other cookie of the origin. A
    /// cookie an answer has since taken away is taken out of the store too,
    /// and at the first page of a launch the store is made to hold what the
    /// jar holds, never the other way round. Nothing is done for a session
    /// that has ended: `session` is the number the web view's store was made
    /// under.
    func ready(_ store: any WebCookies, session: Int) async {
        await myTurn()
        defer { turnOver() }
        guard session == self.session else { return }
        if seen != nil { await collect(from: store, session: session) }
        guard session == self.session else { return }
        let mine = records.filter { isTheSites($0) }
        let keys = Set(mine.map(\.key))
        for cookie in await store.held() {
            let record = CookieRecord(cookie)
            if isTheSites(record), !keys.contains(record.key) { await store.drop(cookie) }
        }
        for record in mine {
            if let cookie = record.cookie { await store.put(cookie) }
        }
        guard session == self.session else { return }
        seen = Dictionary(mine.map { ($0.key, $0.value) }, uniquingKeysWith: { _, last in last })
    }

    /// After a page of `host` finished, when `host` is the origin's own:
    /// takes what the page set, changed or took away in the web view's store
    /// since the jar last looked, and nothing else. A cookie the page left as
    /// it was says nothing, so what an answer set meanwhile stands. Nothing is
    /// taken from a store the jar has not made its own (``ready(_:session:)``),
    /// nor for a session that has ended.
    func adopt(from store: any WebCookies, host: String, session: Int) async {
        guard let site, host.lowercased() == site.host else { return }
        await myTurn()
        defer { turnOver() }
        await collect(from: store, session: session)
    }

    private func collect(from store: any WebCookies, session: Int) async {
        guard session == self.session, let before = seen else { return }
        let found = await store.held().map(CookieRecord.init).filter { isTheSites($0) }
        guard session == self.session else { return }
        let now = Dictionary(found.map { ($0.key, $0) }, uniquingKeysWith: { _, last in last })
        var changed = false
        for (key, record) in now where before[key] != record.value {
            // The page set it, or changed it.
            records.removeAll { $0.key == key }
            if record.expires.map({ $0 > Date() }) ?? true { records.append(record) }
            changed = true
        }
        for (key, value) in before where now[key] == nil {
            // The page took it away: it goes, unless an answer has set another since.
            let held = records.count
            records.removeAll { $0.key == key && $0.value == value }
            changed = changed || records.count != held
        }
        seen = now.mapValues(\.value)
        if changed { save() }
    }

    /// Whether a cookie is one the origin's own host is sent.
    private func isTheSites(_ record: CookieRecord) -> Bool {
        guard let site else { return false }
        return record.matches(host: site.host)
    }

    /// One piece of work on the web view's store at a time, in the order
    /// asked: waits until the work before this has ended.
    private func myTurn() async {
        guard working else {
            working = true
            return
        }
        await withCheckedContinuation { (next: CheckedContinuation<Void, Never>) in
            waiting.append(next)
        }
    }

    /// This piece of work has ended: the next one waiting goes on.
    private func turnOver() {
        if waiting.isEmpty {
            working = false
        } else {
            waiting.removeFirst().resume()
        }
    }

    /// The hosts the jar holds cookies for: the app's origins.
    public var hosts: Set<String> {
        Set(records.map { $0.domain.hasPrefix(".") ? String($0.domain.dropFirst()) : $0.domain }.map { $0.lowercased() })
    }

    // MARK: - The end of a session

    /// The number of the session the jar holds: it moves on each time a
    /// session ends, so what an ended session's answers bring is told apart.
    public private(set) var session = 0

    /// Ends the session the jar holds, at once: its cookies go from memory
    /// and from the Keychain, what an answer or a page of the ended session
    /// brings later is dropped, and the web view's store it knew is never
    /// read again. `false` when the Keychain item could not be deleted.
    @discardableResult
    public func forget() -> Bool {
        records = []
        seen = nil
        session += 1
        return slot?.delete() ?? true
    }

    private func save() {
        guard let slot else { return }
        if records.isEmpty {
            _ = slot.delete()
        } else if let data = try? JSONEncoder().encode(records) {
            slot.write(data)
        }
    }
}

/// Where the jar's master copy is kept: one Keychain item, or a stand-in in tests.
protocol SecretSlot: Sendable {
    func read() -> Data?
    func write(_ data: Data)
    /// Deletes what is kept; `false` when it is still there afterwards.
    func delete() -> Bool
}

/// One Keychain item that never syncs and never moves to another device, and
/// that can be read after the first unlock (so a refresh in the background works).
struct KeychainSlot: SecretSlot {
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

    func delete() -> Bool {
        let status = SecItemDelete(query as CFDictionary)
        if status != errSecSuccess && status != errSecItemNotFound {
            Self.log.error("the session could not be taken out of the Keychain: \(status, privacy: .public)")
        }
        return status == errSecSuccess || status == errSecItemNotFound
    }
}
