import Foundation

/// Loads the pictures a screen shows. A picture of the origin goes with the
/// session's own cookies, from the one cookie store the requests and the web
/// view share, so a picture only a signed-in person may see is seen. A
/// picture from one of the configured picture hosts goes with no cookie, and
/// what it answers is never kept in the jar. No other address is asked for,
/// and no redirect is followed. What it loaded is kept in memory until the
/// session ends.
@MainActor
public final class PictureLoader {
    private let jar: CookieJar?
    private let session: URLSession
    private let kept = NSCache<NSURL, NSData>()
    /// The system's cache of the answers, in memory only.
    private let answers: URLCache
    /// Moves on when the session ends: a picture asked for before is not kept.
    private var era = 0
    /// The origin a picture's address is read against.
    public let site: Origin?
    /// Other hosts pictures may come from, with no cookie.
    public let hosts: [String]

    public init(
        jar: CookieJar?, site: Origin? = nil, hosts: [String] = [], configuration: URLSessionConfiguration = .ephemeral
    ) {
        self.jar = jar
        self.site = site ?? jar?.site
        self.hosts = hosts
        let configuration = HTTPTransport.prepared(configuration, timeout: 30)
        // Pictures may be cached as the system caches any answer.
        let answers = URLCache(memoryCapacity: 8 << 20, diskCapacity: 0)
        self.answers = answers
        configuration.urlCache = answers
        configuration.requestCachePolicy = .useProtocolCachePolicy
        session = URLSession(configuration: configuration, delegate: RedirectRefuser(), delegateQueue: nil)
        kept.totalCostLimit = 32 << 20
    }

    /// The address of a picture the server names, or `nil` when it may not be asked for.
    public func address(_ source: String) -> URL? {
        site?.picture(source, hosts: hosts)
    }

    /// Whether `url` is the origin's own, so the session goes with it.
    private func isOurs(_ url: URL) -> Bool {
        site?.inside(url.absoluteString) != nil
    }

    /// The request for a picture: the session's cookies only when it is the origin's.
    public func request(_ url: URL) -> URLRequest {
        var request = URLRequest(url: url)
        request.setValue("image/*", forHTTPHeaderField: "Accept")
        if isOurs(url), let cookie = jar?.header(for: url) { request.setValue(cookie, forHTTPHeaderField: "Cookie") }
        return request
    }

    /// The picture's bytes, or nothing when it could not be had.
    public func load(_ url: URL) async -> Data? {
        if let data = kept.object(forKey: url as NSURL) { return data as Data }
        guard address(url.absoluteString) != nil else { return nil }
        let request = request(url)
        let era = era
        let asked = jar?.session
        guard let (data, response) = try? await session.data(for: request),
            let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode), era == self.era
        else { return nil }
        if isOurs(url) { jar?.take(from: http, for: url, session: asked) }
        kept.setObject(data as NSData, forKey: url as NSURL, cost: data.count)
        return data
    }

    /// The session ended: every picture kept, in memory and in the system's
    /// cache, goes, and a picture still on its way is not kept.
    public func forget() {
        era += 1
        kept.removeAllObjects()
        answers.removeAllCachedResponses()
    }
}
