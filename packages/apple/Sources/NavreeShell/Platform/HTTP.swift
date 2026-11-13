import Foundation
import NavreeRuntime

/// What answers the runtime core's requests and opens its event streams.
/// The network, in an app; answers kept with it, in a demo or a test.
@MainActor
public protocol Transport: AnyObject {
    /// Sends exactly this request, and answers whatever comes back.
    func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void)
    func cancel(_ request: Int)
    /// Opens a server-sent event stream; reports it open, each event's name,
    /// and its end. Never reconnects by itself.
    func open(stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void)
    func close(stream: Int)
    /// The cookie store the web view shares, when there is one.
    var jar: CookieJar? { get }
}

/// Carries out the core's `fetch` effects with `URLSession`: exactly the
/// headers it gives, the jar's cookies, `x-xsrf-token` when it asks, no
/// redirect followed (a 3xx goes back to the core as it came, after its
/// cookies are kept), and every answer handed back whatever its status, with
/// the `x-navree-*` headers.
///
/// Whether an address may be asked at all is the core's to decide: this sends
/// what it is told, and nothing of its own.
@MainActor
public final class HTTPTransport: Transport {
    public let jar: CookieJar?
    private let cookies: CookieJar
    private let session: URLSession
    private let streams: URLSession
    private var tasks: [Int: Task<Void, Never>] = [:]
    private var open: [Int: Task<Void, Never>] = [:]

    /// `configuration` lets a test answer requests itself (its `protocolClasses`);
    /// cookies, caching and redirects are always this class's own.
    public init(jar: CookieJar, configuration: URLSessionConfiguration = .ephemeral) {
        self.jar = jar
        cookies = jar
        session = URLSession(configuration: Self.prepared(configuration, timeout: 60), delegate: RedirectRefuser(), delegateQueue: nil)
        // A stream may be quiet for as long as it likes.
        streams = URLSession(configuration: Self.prepared(configuration, timeout: 24 * 60 * 60), delegate: RedirectRefuser(), delegateQueue: nil)
    }

    nonisolated static func prepared(_ base: URLSessionConfiguration, timeout: TimeInterval) -> URLSessionConfiguration {
        let configuration = base.copy() as? URLSessionConfiguration ?? .ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = timeout
        return configuration
    }

    /// The request as it goes over the wire, or `nil` for an address that is not one.
    public func urlRequest(for request: FetchRequest) -> URLRequest? {
        guard let url = URL(string: request.url), url.scheme != nil else { return nil }
        var urlRequest = URLRequest(url: url)
        urlRequest.httpMethod = request.method.uppercased()
        urlRequest.timeoutInterval = TimeInterval(max(request.timeoutMs, 1)) / 1000
        for (name, value) in request.headers {
            urlRequest.setValue(value, forHTTPHeaderField: name)
        }
        if let cookie = cookies.header(for: url) {
            urlRequest.setValue(cookie, forHTTPHeaderField: "Cookie")
        }
        if request.xsrf, let token = cookies.xsrfToken(for: url) {
            urlRequest.setValue(token, forHTTPHeaderField: "x-xsrf-token")
        }
        switch request.body {
        case .form(let text)?, .json(let text)?:
            // Sent even when empty: the content type is already in the headers.
            urlRequest.httpBody = Data(text.utf8)
        case .multipart(let parts)?:
            let boundary = "navree-" + UUID().uuidString
            urlRequest.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
            urlRequest.httpBody = Multipart.body(parts, boundary: boundary)
        case nil:
            if request.method.lowercased() != "get" { urlRequest.httpBody = Data() }
        }
        return urlRequest
    }

    public func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        guard let urlRequest = urlRequest(for: request), let url = urlRequest.url else {
            return answer(.failed(request.request, .other))
        }
        let number = request.request
        let session = session
        tasks[number] = Task { [weak self] in
            let outcome: Result<(Data, URLResponse), any Error>
            do {
                outcome = .success(try await session.data(for: urlRequest))
            } catch {
                outcome = .failure(error)
            }
            guard let self, self.tasks.removeValue(forKey: number) != nil else { return }
            switch outcome {
            case .success(let (data, response)):
                guard let http = response as? HTTPURLResponse else { return answer(.failed(number, .other)) }
                self.cookies.take(from: http, for: url)
                answer(Self.response(number, http, data))
            case .failure(let error):
                if (error as? URLError)?.code == .cancelled || Task.isCancelled { return }
                answer(.failed(number, Self.failure(error)))
            }
        }
    }

    public func cancel(_ request: Int) {
        tasks.removeValue(forKey: request)?.cancel()
    }

    /// The `response` event for an answer: a redirect as it came, anything
    /// else with its body as text.
    nonisolated static func response(_ request: Int, _ http: HTTPURLResponse, _ data: Data) -> Response {
        let headers = http.allHeaderFields.compactMap { key, value -> [String]? in
            guard let name = (key as? String)?.lowercased(), name.hasPrefix("x-navree-") else { return nil }
            return [name, value as? String ?? "\(value)"]
        }.sorted { $0[0] < $1[0] }
        let redirect = Response.isRedirect(http.statusCode)
        return Response(
            request: request, status: http.statusCode, contentType: http.value(forHTTPHeaderField: "Content-Type"),
            location: redirect ? http.value(forHTTPHeaderField: "Location") : nil, headers: headers,
            body: redirect ? "" : String(decoding: data, as: UTF8.self)
        )
    }

    /// No answer, in the core's words.
    nonisolated static func failure(_ error: any Error) -> Response.TransportError {
        switch (error as? URLError)?.code {
        case .notConnectedToInternet?, .networkConnectionLost?, .cannotFindHost?, .cannotConnectToHost?,
            .dnsLookupFailed?, .dataNotAllowed?, .internationalRoamingOff?:
            .offline
        case .timedOut?:
            .timeout
        default:
            .other
        }
    }

    // MARK: - Event streams

    public func open(
        stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) {
        guard let address = URL(string: url) else { return report(.closed(status: nil)) }
        var request = URLRequest(url: address)
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        for (name, value) in headers { request.setValue(value, forHTTPHeaderField: name) }
        if let cookie = cookies.header(for: address) { request.setValue(cookie, forHTTPHeaderField: "Cookie") }
        let streams = streams
        open[stream] = Task { [weak self] in
            var status: Int?
            do {
                let (bytes, response) = try await streams.bytes(for: request)
                status = (response as? HTTPURLResponse)?.statusCode
                if status == 200 {
                    report(.open)
                    // Lines by hand: an event ends at a blank line, which a sequence of lines leaves out.
                    var reader = EventStreamReader()
                    var line: [UInt8] = []
                    for try await byte in bytes {
                        guard byte == 0x0A else {
                            line.append(byte)
                            continue
                        }
                        if line.last == 0x0D { line.removeLast() }
                        if let event = reader.read(String(decoding: line, as: UTF8.self)) {
                            report(.event(name: event))
                        }
                        line.removeAll(keepingCapacity: true)
                    }
                }
            } catch {}
            // Ended or failed; said only when the core did not close it.
            guard let self, self.open.removeValue(forKey: stream) != nil else { return }
            report(.closed(status: status))
        }
    }

    public func close(stream: Int) {
        open.removeValue(forKey: stream)?.cancel()
    }
}

/// Reads a server-sent event stream a line at a time: an event is
/// dispatched at the blank line after it, by its `event:` name, and only if
/// it had data. The data itself is never kept.
struct EventStreamReader {
    private var name: String?
    private var data = false

    /// The event a line ends, as `.some(name)` (a nil name for an event with none).
    mutating func read(_ line: String) -> String?? {
        if line.isEmpty {
            defer { name = nil; data = false }
            return data ? .some(name) : nil
        }
        if line.hasPrefix(":") { return nil }
        if line.hasPrefix("event:") {
            let value = line.dropFirst("event:".count).trimmingCharacters(in: .whitespaces)
            name = value.isEmpty ? nil : value
        } else if line.hasPrefix("data") {
            data = true
        }
        return nil
    }
}

/// `multipart/form-data`, the parts in order. A file's bytes are the ones the
/// client keeps under its token; this build keeps none, so a file part goes empty.
enum Multipart {
    static func body(_ parts: [FetchRequest.Part], boundary: String) -> Data {
        var body = Data()
        func line(_ text: String) { body.append(Data((text + "\r\n").utf8)) }
        for part in parts {
            line("--\(boundary)")
            switch part {
            case .text(let name, let value):
                line("Content-Disposition: form-data; name=\"\(escape(name))\"")
                line("")
                line(value)
            case .file(let name, _, let filename, let type):
                line("Content-Disposition: form-data; name=\"\(escape(name))\"; filename=\"\(escape(filename))\"")
                line("Content-Type: \(type)")
                line("")
                line("")
            }
        }
        line("--\(boundary)--")
        return body
    }

    static func escape(_ text: String) -> String {
        text.replacingOccurrences(of: "\"", with: "%22").replacingOccurrences(of: "\r", with: "%0D")
            .replacingOccurrences(of: "\n", with: "%0A")
    }
}

/// Stops every redirect, so the core sees the answer that carried it: its
/// cookies are kept, and where it leads is the core's to decide.
final class RedirectRefuser: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest
    ) async -> URLRequest? {
        nil
    }
}
