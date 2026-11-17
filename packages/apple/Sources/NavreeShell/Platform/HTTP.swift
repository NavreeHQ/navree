import Foundation
import NavreeProtocol
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
    /// The session ended: every request and stream under way is dropped, and
    /// none of them is answered or reported.
    func stop()
}

extension Transport {
    /// A transport with nothing under way has nothing to drop.
    public func stop() {}
}

/// Carries out the core's `fetch` effects with `URLSession`: exactly the
/// headers it gives, the jar's cookies, `x-xsrf-token` when it asks, no
/// redirect followed (a 3xx goes back to the core as it came, after its
/// cookies are kept), and every answer handed back whatever its status, with
/// the `x-navree-*` headers and `cache-control`. An answer's body is read up to the protocol's
/// limit (``NavreeProtocol/Wire/maxBody``) and no further: a longer one is
/// answered as a request that failed, as the core's header asks of a client.
///
/// Whether an address may be asked at all is the core's to decide: this sends
/// what it is told, and nothing of its own. It asks one thing again itself:
/// an address that is not the origin's own, whole, is never sent, and is
/// answered as a request that failed.
@MainActor
public final class HTTPTransport: Transport {
    public let jar: CookieJar?
    private let cookies: CookieJar
    /// Where the files a form sends are kept.
    private let files: FileStore
    private let session: URLSession
    private let streams: URLSession
    private var tasks: [Int: Task<Void, Never>] = [:]
    private var open: [Int: Task<Void, Never>] = [:]

    /// `configuration` lets a test answer requests itself (its `protocolClasses`);
    /// cookies, caching and redirects are always this class's own.
    public init(jar: CookieJar, configuration: URLSessionConfiguration = .ephemeral, files: FileStore = .shared) {
        self.jar = jar
        cookies = jar
        self.files = files
        session = URLSession(configuration: Self.prepared(configuration, timeout: 60), delegate: RedirectRefuser(), delegateQueue: nil)
        // A stream may be quiet for as long as it likes.
        streams = URLSession(configuration: Self.prepared(configuration, timeout: 24 * 60 * 60), delegate: RedirectRefuser(), delegateQueue: nil)
    }

    deinit {
        // The system holds a session with a delegate until it is ended: a
        // window's transport that is let go ends its two, and what they had
        // under way with them.
        session.invalidateAndCancel()
        streams.invalidateAndCancel()
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

    /// The request as it goes over the wire, or `nil` for an address that is
    /// not the origin's own, a header that cannot be sent as it is, or a file
    /// whose bytes the app no longer holds.
    public func urlRequest(for request: FetchRequest) -> URLRequest? {
        guard let url = cookies.site?.inside(request.url) else { return nil }
        let cookie = cookies.header(for: url)
        let token = request.xsrf ? cookies.xsrfToken(for: url) : nil
        var headers = request.headers
        if let cookie { headers.append(("Cookie", cookie)) }
        if let token { headers.append(("x-xsrf-token", token)) }
        guard Self.areSendable(headers) else { return nil }
        var urlRequest = URLRequest(url: url)
        urlRequest.httpMethod = request.method.uppercased()
        urlRequest.timeoutInterval = TimeInterval(max(request.timeoutMs, 1)) / 1000
        for (name, value) in headers {
            urlRequest.setValue(value, forHTTPHeaderField: name)
        }
        switch request.body {
        case .form(let text)?, .json(let text)?:
            // Sent even when empty: the content type is already in the headers.
            urlRequest.httpBody = Data(text.utf8)
        case .multipart(let parts)?:
            let boundary = "navree-" + UUID().uuidString
            // A file whose bytes are gone is not sent empty under its name,
            // for the server to store and call saved: the request does not go.
            guard let body = Multipart.body(parts, boundary: boundary, files: files.data) else { return nil }
            urlRequest.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
            urlRequest.httpBody = body
        case nil:
            if request.method.lowercased() != "get" { urlRequest.httpBody = Data() }
        }
        return urlRequest
    }

    /// Whether every header can go on the wire as it is: a name of visible
    /// ASCII with no colon, and a value of printable ASCII, spaces and tabs.
    /// Anything else (a letter of another script, a line break) is never
    /// sent, and the request is answered as one that failed.
    nonisolated static func areSendable(_ headers: [(String, String)]) -> Bool {
        headers.allSatisfy { name, value in
            !name.isEmpty && name.utf8.allSatisfy { (0x21...0x7E).contains($0) && $0 != 0x3A }
                && value.utf8.allSatisfy { (0x20...0x7E).contains($0) || $0 == 0x09 }
        }
    }

    public func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        guard let urlRequest = urlRequest(for: request), let url = urlRequest.url else {
            return answer(.failed(request.request, .other))
        }
        let number = request.request
        let session = session
        // The cookies an answer sets belong to the session it was asked in.
        let asked = cookies.session
        tasks[number] = Task { [weak self] in
            let outcome: Result<(Data?, URLResponse), any Error>
            do {
                outcome = .success(try await Self.read(urlRequest, with: session, most: Wire.maxBody))
            } catch {
                outcome = .failure(error)
            }
            guard let self, self.tasks.removeValue(forKey: number) != nil else { return }
            switch outcome {
            case .success(let (data, response)):
                guard let http = response as? HTTPURLResponse else { return answer(.failed(number, .other)) }
                self.cookies.take(from: http, for: url, session: asked)
                // A body past the protocol's limit was never read to its end:
                // it is answered as any answer that could not be read.
                guard let data else { return answer(.failed(number, .other)) }
                answer(Self.response(number, http, data))
            case .failure(let error):
                if (error as? URLError)?.code == .cancelled || Task.isCancelled { return }
                answer(.failed(number, Self.failure(error)))
            }
        }
    }

    /// Asks `request` and reads the answer's body, at most `most` bytes of
    /// it, away from the main actor. A body that announces more than `most`
    /// (its `Content-Length`) is not read at all, and one that turns out
    /// longer is given up at the byte past the limit: either way the body
    /// comes back as `nil` and the load is cancelled, so an answer of any
    /// size costs no more memory than the limit.
    nonisolated static func read(
        _ request: URLRequest, with session: URLSession, most: Int
    ) async throws -> (Data?, URLResponse) {
        let (bytes, response) = try await session.bytes(for: request)
        let announced = response.expectedContentLength
        if announced > Int64(most) {
            bytes.task.cancel()
            return (nil, response)
        }
        var body: [UInt8] = []
        if announced > 0 { body.reserveCapacity(Int(announced)) }
        for try await byte in bytes {
            if body.count >= most {
                bytes.task.cancel()
                return (nil, response)
            }
            body.append(byte)
        }
        return (Data(body), response)
    }

    public func cancel(_ request: Int) {
        tasks.removeValue(forKey: request)?.cancel()
    }

    /// Drops every request and stream under way. Each task finds itself
    /// gone from the list when it ends, and says nothing.
    public func stop() {
        for task in tasks.values { task.cancel() }
        tasks.removeAll()
        for task in open.values { task.cancel() }
        open.removeAll()
    }

    /// The `response` event for an answer: a redirect as it came, anything
    /// else with its body as text. Its `x-navree-` headers and its
    /// `cache-control` go with it (names in lower case, values as they came),
    /// and no other header: the core keeps a page only when its
    /// `cache-control` does not say `no-store`.
    nonisolated static func response(_ request: Int, _ http: HTTPURLResponse, _ data: Data) -> Response {
        let headers = http.allHeaderFields.compactMap { key, value -> [String]? in
            guard let name = (key as? String)?.lowercased(), name.hasPrefix("x-navree-") || name == "cache-control"
            else { return nil }
            return [name, value as? String ?? "\(value)"]
        }.sorted { $0[0] < $1[0] }
        let redirect = Response.isRedirect(http.statusCode)
        return Response(
            request: request, status: http.statusCode, contentType: http.value(forHTTPHeaderField: "Content-Type"),
            location: redirect ? http.value(forHTTPHeaderField: "Location") : nil, headers: headers,
            body: redirect ? "" : String(decoding: data, as: UTF8.self)
        )
    }

    /// No answer, in the core's words. `offline` says one thing only: nothing
    /// of the request was sent, so a write it carried was never made, and the
    /// core counts it so. It is said only for the codes whose own meaning is
    /// that no connection was made:
    ///
    /// - `notConnectedToInternet`: no internet connection is up, and none can be brought up.
    /// - `cannotFindHost`, and `dnsLookupFailed` (which the system no longer
    ///   uses, and means the same): the host's name was not resolved.
    /// - `cannotConnectToHost`: the attempt to connect failed.
    /// - `dataNotAllowed`: the cellular network disallowed a connection.
    /// - `internationalRoamingOff`: the connection needed roaming, which is off.
    /// - `callIsActive`: a connection was tried during a call on a network
    ///   that carries no data beside one.
    ///
    /// `timedOut` is `timeout`. Everything else is `other`, and so may have
    /// been sent: above all `networkConnectionLost`, a connection severed in
    /// the middle of a load, after the request may have gone out; and a
    /// secure connection that failed, which the system does not say failed
    /// before anything was sent.
    nonisolated static func failure(_ error: any Error) -> Response.TransportError {
        switch (error as? URLError)?.code {
        case .notConnectedToInternet?, .cannotFindHost?, .dnsLookupFailed?, .cannotConnectToHost?, .dataNotAllowed?,
            .internationalRoamingOff?, .callIsActive?:
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
        guard let address = cookies.site?.inside(url) else { return report(.closed(status: nil)) }
        var sent = [("Accept", "text/event-stream")] + headers
        if let cookie = cookies.header(for: address) { sent.append(("Cookie", cookie)) }
        guard Self.areSendable(sent) else { return report(.closed(status: nil)) }
        var request = URLRequest(url: address)
        for (name, value) in sent { request.setValue(value, forHTTPHeaderField: name) }
        let streams = streams
        open[stream] = Task { [weak self] in
            // Read away from the main actor: a stream that sends fast does not hold up the app.
            let status = await Self.listen(to: request, with: streams) { [weak self] state in
                // Said only while the core holds the stream open.
                guard let self, self.open[stream] != nil else { return }
                report(state)
            }
            // Ended or failed; said only when the core did not close it.
            guard let self, self.open.removeValue(forKey: stream) != nil else { return }
            report(.closed(status: status))
        }
    }

    /// Reads a stream until it ends or fails, and says it is open and each
    /// event's name. Gives the answer's status, when there was an answer.
    /// It runs away from the main actor, and holds at most one line's worth
    /// of the stream (``LineCutter``).
    nonisolated static func listen(
        to request: URLRequest, with session: URLSession, report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) async -> Int? {
        var status: Int?
        do {
            let (bytes, response) = try await session.bytes(for: request)
            status = (response as? HTTPURLResponse)?.statusCode
            guard status == 200 else { return status }
            await report(.open)
            // Lines by hand: an event ends at a blank line, which a sequence of lines leaves out.
            var reader = EventStreamReader()
            var lines = LineCutter()
            for try await byte in bytes {
                guard let line = lines.take(byte), let event = reader.read(line) else { continue }
                await report(.event(name: event))
            }
        } catch {}
        return status
    }

    public func close(stream: Int) {
        open.removeValue(forKey: stream)?.cancel()
    }
}

/// Cuts a stream's bytes into lines. It holds at most ``most`` bytes of a
/// line: of a longer one only that start is kept and given, so a stream
/// that never ends a line costs no more memory than one line may. What a
/// stream is read for is each event's name, a few bytes at the start of a
/// line; the data, which may be long, is never read.
struct LineCutter {
    /// The most of one line that is held.
    static let most = 64 * 1024

    private let limit: Int
    private var line: [UInt8] = []

    init(most: Int = LineCutter.most) {
        limit = most
    }

    /// Takes the stream's next byte. Gives the line it ends, without its
    /// line break; `nil` while the line goes on.
    mutating func take(_ byte: UInt8) -> String? {
        guard byte == 0x0A else {
            if line.count < limit { line.append(byte) }
            return nil
        }
        defer { line.removeAll(keepingCapacity: true) }
        if line.last == 0x0D { line.removeLast() }
        return String(decoding: line, as: UTF8.self)
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
/// client keeps under its token. With a token it no longer holds (the person
/// signed out in another window, the system emptied the folder) there is no
/// body at all: `nil`, never a part with the file's name and no bytes.
enum Multipart {
    static func body(_ parts: [FetchRequest.Part], boundary: String, files: (String) -> Data? = { _ in nil }) -> Data? {
        var body = Data()
        func line(_ text: String) { body.append(Data((text + "\r\n").utf8)) }
        for part in parts {
            line("--\(boundary)")
            switch part {
            case .text(let name, let value):
                line("Content-Disposition: form-data; name=\"\(escape(name))\"")
                line("")
                line(value)
            case .file(let name, let token, let filename, let type):
                guard let bytes = files(token) else { return nil }
                line("Content-Disposition: form-data; name=\"\(escape(name))\"; filename=\"\(escape(filename))\"")
                line("Content-Type: \(type.isEmpty ? "application/octet-stream" : type)")
                line("")
                body.append(bytes)
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
