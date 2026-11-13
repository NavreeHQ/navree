import Foundation

/// Loads the pictures a screen shows with the session's own cookies, from the
/// one cookie store the requests and the web view share, so a picture only a
/// signed-in person may see is seen. Never follows a redirect to another
/// host with them; keeps what it loaded in memory for as long as the app runs.
@MainActor
public final class PictureLoader {
    private let jar: CookieJar?
    private let session: URLSession
    private let kept = NSCache<NSURL, NSData>()

    public init(jar: CookieJar?, configuration: URLSessionConfiguration = .ephemeral) {
        self.jar = jar
        let configuration = HTTPTransport.prepared(configuration, timeout: 30)
        // Pictures may be cached as the system caches any answer.
        configuration.urlCache = URLCache(memoryCapacity: 8 << 20, diskCapacity: 0)
        configuration.requestCachePolicy = .useProtocolCachePolicy
        session = URLSession(configuration: configuration, delegate: RedirectRefuser(), delegateQueue: nil)
        kept.totalCostLimit = 32 << 20
    }

    /// The request for a picture: its cookies from the jar (only those of its own site).
    public func request(_ url: URL) -> URLRequest {
        var request = URLRequest(url: url)
        request.setValue("image/*", forHTTPHeaderField: "Accept")
        if let cookie = jar?.header(for: url) { request.setValue(cookie, forHTTPHeaderField: "Cookie") }
        return request
    }

    /// The picture's bytes, or nothing when it could not be had.
    public func load(_ url: URL) async -> Data? {
        if let data = kept.object(forKey: url as NSURL) { return data as Data }
        guard url.scheme == "https" || url.scheme == "http" else { return nil }
        let request = request(url)
        guard let (data, response) = try? await session.data(for: request),
            let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode)
        else { return nil }
        jar?.take(from: http, for: url)
        kept.setObject(data as NSData, forKey: url as NSURL, cost: data.count)
        return data
    }
}
