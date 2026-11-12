import Foundation
import NavreeProtocol

/// A request the app makes to its origin.
public struct OriginRequest: Sendable, Equatable {
    public var method: Method
    /// The path and query on the origin: `/orders?sort=total`.
    public var address: String
    /// The values a post sends, in order.
    public var form: [Pair]

    public init(method: Method = .get, address: String, form: [Pair] = []) {
        self.method = method
        self.address = address
        self.form = form
    }
}

/// What came back from the origin.
public enum OriginReply: Sendable, Equatable {
    /// A body, with its status and media type: an envelope or the shell
    /// document (a refused form is a 422 and a missing page a 404, both with an
    /// envelope to draw), or something else, which is the web view's.
    case body(status: Int, contentType: String?, data: Data)
    /// A redirect within the app: the client asks for it next.
    case redirect(String)
    /// A redirect out of the app: the system browser opens it.
    case leave(URL)
    /// No answer: no network, a timeout, a refused connection.
    case failed(String)

    /// The envelope in the body, when it is one.
    public var envelope: Envelope? {
        guard case .body(_, let contentType, let data) = self, Self.isNative(contentType) else { return nil }
        return try? WireDecoder.decode(Envelope.self, from: data)
    }

    /// The shell document in the body, when it is one.
    public var shell: ShellDocument? {
        guard case .body(_, let contentType, let data) = self, Self.isNative(contentType) else { return nil }
        return try? WireDecoder.decode(ShellDocument.self, from: data)
    }

    public var status: Int? {
        if case .body(let status, _, _) = self { return status }
        return nil
    }

    static func isNative(_ contentType: String?) -> Bool {
        contentType?.lowercased().hasPrefix(Wire.contentType) ?? false
    }
}

/// Talks to the origin as a native client: the native headers on every
/// request, the session from the cookie jar, forms encoded as a browser
/// encodes them, and redirects never followed blindly.
@MainActor
public final class Transport {
    public let base: URL
    public let origin: Origin
    public let descriptor: ClientDescriptor
    public let jar: CookieJar
    private let session: URLSession
    private let redirects = RedirectRefuser()

    /// `configuration` lets a test or a demo answer requests itself (its
    /// `protocolClasses`); cookies and caching are always this class's own.
    public init(
        base: URL, origin: Origin, descriptor: ClientDescriptor, jar: CookieJar,
        configuration: URLSessionConfiguration = .ephemeral
    ) {
        self.base = base
        self.origin = origin
        self.descriptor = descriptor
        self.jar = jar
        let configuration = configuration
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 30
        session = URLSession(configuration: configuration, delegate: redirects, delegateQueue: nil)
    }

    /// The request as it goes over the wire.
    public func urlRequest(for request: OriginRequest) -> URLRequest? {
        guard var url = URL(string: request.address, relativeTo: base)?.absoluteURL, origin.contains(url) else {
            return nil
        }
        if request.method == .get, !request.form.isEmpty {
            var components = URLComponents(url: url, resolvingAgainstBaseURL: true)
            components?.percentEncodedQuery = FormEncoding.encode(request.form)
            url = components?.url ?? url
        }
        var urlRequest = URLRequest(url: url)
        urlRequest.httpMethod = request.method == .post ? "POST" : "GET"
        urlRequest.setValue(String(Wire.major), forHTTPHeaderField: Wire.nativeHeader)
        urlRequest.setValue(descriptor.header, forHTTPHeaderField: Wire.clientHeader)
        urlRequest.setValue(Wire.contentType, forHTTPHeaderField: "Accept")
        if let cookie = jar.header(for: url) {
            urlRequest.setValue(cookie, forHTTPHeaderField: "Cookie")
        }
        if request.method == .post {
            // Always, even with no fields: the server refuses a post without it.
            urlRequest.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
            urlRequest.httpBody = Data(FormEncoding.encode(request.form).utf8)
        }
        return urlRequest
    }

    /// Sends `request` and reads what comes back.
    public func send(_ request: OriginRequest) async -> OriginReply {
        guard let urlRequest = urlRequest(for: request), let url = urlRequest.url else {
            return .failed("The address is not on the app's origin.")
        }
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: urlRequest)
        } catch {
            return .failed(error.localizedDescription)
        }
        guard let http = response as? HTTPURLResponse else { return .failed("No HTTP answer.") }
        jar.take(from: http, for: url)
        return Self.read(
            status: http.statusCode, location: http.value(forHTTPHeaderField: "Location"),
            contentType: http.value(forHTTPHeaderField: "Content-Type"), data: data, from: url, origin: origin
        )
    }

    /// Reads an answer; kept apart from the network so it can be tested.
    nonisolated static func read(
        status: Int, location: String?, contentType: String?, data: Data, from url: URL, origin: Origin
    ) -> OriginReply {
        if (300..<400).contains(status), let location {
            switch origin.destination(of: location, from: url) {
            case .app(let address): return .redirect(address)
            case .external(let target): return .leave(target)
            case .refused: return .failed("The server sent the app somewhere it will not go.")
            }
        }
        return .body(status: status, contentType: contentType, data: data)
    }
}

/// Stops every redirect, so the client sees the answer that carried it: its
/// cookies are kept, and a redirect to another origin is never followed.
final class RedirectRefuser: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest
    ) async -> URLRequest? {
        nil
    }
}

/// `application/x-www-form-urlencoded`, exactly as a browser writes it.
public enum FormEncoding {
    public static func encode(_ pairs: [Pair]) -> String {
        pairs.map { escape($0.name) + "=" + escape($0.value) }.joined(separator: "&")
    }

    static func escape(_ text: String) -> String {
        var out = ""
        for byte in text.utf8 {
            switch byte {
            case 0x2A, 0x2D, 0x2E, 0x5F, 0x30...0x39, 0x41...0x5A, 0x61...0x7A:
                out.unicodeScalars.append(Unicode.Scalar(byte))
            case 0x20:
                out.append("+")
            default:
                out.append(String(format: "%%%02X", byte))
            }
        }
        return out
    }
}
