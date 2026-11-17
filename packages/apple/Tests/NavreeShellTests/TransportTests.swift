import Foundation
import NavreeCoreBridge
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// An origin that answers from the test: every request it saw, and what it answers.
final class StubOrigin: URLProtocol, @unchecked Sendable {
    struct Answer: Sendable {
        var status: Int
        var headers: [String: String]
        var body: String
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var answers: [String: Answer] = [:]
    nonisolated(unsafe) private static var seen: [String: [URLRequest]] = [:]

    /// What requests to `host` are answered with.
    static func answer(_ host: String, _ answer: Answer) {
        lock.withLock { answers[host] = answer }
    }

    static func seen(_ host: String) -> [URLRequest] {
        lock.withLock { seen[host] ?? [] }
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url, let host = url.host else { return }
        var request = self.request
        if request.httpBody == nil, let stream = request.httpBodyStream {
            stream.open()
            var data = Data()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                data.append(buffer, count: count)
            }
            stream.close()
            request.httpBody = data
        }
        let answer = Self.lock.withLock {
            Self.seen[host, default: []].append(request)
            return Self.answers[host]
        } ?? Answer(status: 404, headers: [:], body: "")
        let response = HTTPURLResponse(url: url, statusCode: answer.status, httpVersion: "HTTP/1.1", headerFields: answer.headers)!
        if (300..<400).contains(answer.status), let location = answer.headers["Location"],
            let next = URL(string: location, relativeTo: url)
        {
            // As the network would: the system asks whether to follow it.
            client?.urlProtocol(self, wasRedirectedTo: URLRequest(url: next), redirectResponse: response)
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(answer.body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}

    static var configuration: URLSessionConfiguration {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [StubOrigin.self]
        return configuration
    }
}

@MainActor
func fetched(_ transport: HTTPTransport, _ request: FetchRequest) async -> Response {
    await withCheckedContinuation { done in
        transport.fetch(request) { done.resume(returning: $0) }
    }
}

@MainActor
struct HTTPTests {
    @Test func aRedirectIsHandedToTheCoreAsItCameAndItsCookiesAreKept() async throws {
        let host = "redirect.shop.example"
        StubOrigin.answer(host, .init(
            status: 303,
            headers: ["Location": "/orders/1", "Set-Cookie": "session=s3cret; Path=/; Secure; HttpOnly", "X-Navree-Shell": "9f3c"],
            body: ""
        ))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let response = await fetched(transport, FetchRequest(request: 5, method: "post", url: "https://\(host)/orders/1/refund", body: .form("")))
        #expect(response.status == 303)
        #expect(response.location == "/orders/1")
        #expect(response.body == "")
        #expect(response.headers == [["x-navree-shell", "9f3c"]])
        #expect(StubOrigin.seen(host).count == 1, "the redirect was followed")
        #expect(jar.header(for: URL(string: "https://\(host)/orders/1")!) == "session=s3cret")
    }

    @Test func aRequestCarriesExactlyItsHeadersTheSessionAndTheXSRFTokenWhenAsked() async throws {
        let host = "headers.shop.example"
        StubOrigin.answer(host, .init(status: 422, headers: ["Content-Type": "application/vnd.navree.native+json"], body: #"{"navree":1}"#))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        jar.merge([
            CookieRecord(name: "session", value: "s3cret", domain: host),
            CookieRecord(name: "XSRF-TOKEN", value: "tk%3D91c2", domain: host, httpOnly: false),
        ])
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let request = FetchRequest(
            request: 7, method: "post", url: "https://\(host)/orders/1/refund",
            headers: [
                ("x-navree-native", "1"), ("x-navree-client", "platform=ios; form=phone"),
                ("accept", "application/vnd.navree.native+json"), ("content-type", "application/x-www-form-urlencoded"),
            ],
            body: .form("amount=0&reason=late"), timeoutMs: 60_000, xsrf: true
        )
        let response = await fetched(transport, request)
        #expect(response.status == 422)
        #expect(response.body == #"{"navree":1}"#)
        #expect(response.contentType == "application/vnd.navree.native+json")
        let sent = try #require(StubOrigin.seen(host).last)
        #expect(sent.httpMethod == "POST")
        #expect(sent.value(forHTTPHeaderField: "x-navree-native") == "1")
        #expect(sent.value(forHTTPHeaderField: "x-navree-client") == "platform=ios; form=phone")
        #expect(sent.value(forHTTPHeaderField: "content-type") == "application/x-www-form-urlencoded")
        #expect(sent.value(forHTTPHeaderField: "x-xsrf-token") == "tk=91c2")
        #expect(sent.value(forHTTPHeaderField: "Cookie")?.contains("session=s3cret") == true)
        #expect(sent.httpBody.map { String(decoding: $0, as: UTF8.self) } == "amount=0&reason=late")
        #expect(sent.timeoutInterval == 60)

        // Without `xsrf`, no token goes; a post with no body still sends one, empty.
        let plain = try #require(transport.urlRequest(for: FetchRequest(request: 8, method: "delete", url: "https://\(host)/x")))
        #expect(plain.value(forHTTPHeaderField: "x-xsrf-token") == nil)
        #expect(plain.httpMethod == "DELETE")
        #expect(plain.httpBody == Data())
        let get = try #require(transport.urlRequest(for: FetchRequest(request: 9, url: "https://\(host)/orders")))
        #expect(get.httpBody == nil)
    }

    @Test func anAnswerThatAnnouncesMoreThanTheProtocolAllowsIsNotReadAndIsAnsweredAsAFailure() async throws {
        let host = "export.shop.example"
        // An export of a gigabyte, linked from a page of the app.
        StubOrigin.answer(host, .init(status: 200, headers: ["Content-Length": "1073741824"], body: "{}"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let response = await fetched(transport, FetchRequest(request: 3, url: "https://\(host)/exports/orders.json"))
        #expect(response.error == "other")
        #expect(response.status == 0)
        #expect(response.body.isEmpty)
    }

    @Test func aBodyIsReadUpToTheLimitAndGivenUpAtTheByteAfterIt() async throws {
        let host = "long.shop.example"
        let session = URLSession(configuration: StubOrigin.configuration)
        let request = URLRequest(url: try #require(URL(string: "https://\(host)/backup")))
        // A body that does not say how long it is: counted as it arrives.
        StubOrigin.answer(host, .init(status: 200, headers: [:], body: String(repeating: "x", count: 4097)))
        let pastTheLimit = try await HTTPTransport.read(request, with: session, most: 4096).0
        let atTheLimit = try await HTTPTransport.read(request, with: session, most: 4097).0
        #expect(pastTheLimit == nil)
        #expect(atTheLimit?.count == 4097)
        // One that says so is refused before a byte of it is kept.
        StubOrigin.answer(host, .init(status: 200, headers: ["Content-Length": "4097"], body: String(repeating: "x", count: 4097)))
        let announcedPast = try await HTTPTransport.read(request, with: session, most: 4096).0
        let announcedAt = try await HTTPTransport.read(request, with: session, most: 4097).0
        #expect(announcedPast == nil)
        #expect(announcedAt?.count == 4097)
        // A picture larger than a picture may be is not shown.
        StubOrigin.answer(host, .init(status: 200, headers: ["Content-Length": "\(PictureLoader.mostBytes + 1)"], body: "png"))
        let pictures = PictureLoader(jar: CookieJar(origin: "https://\(host)", persistent: false), configuration: StubOrigin.configuration)
        let poster = try #require(URL(string: "https://\(host)/storage/poster.png"))
        let shown = await pictures.load(poster)
        #expect(shown == nil)
    }

    @Test func theLimitIsTheProtocolsOwn() throws {
        let source = Fixtures.repository.appendingPathComponent("crates/navree-native/src/lib.rs")
        let rust = try String(contentsOf: source, encoding: .utf8)
        #expect(rust.contains("pub const MAX_BODY: usize = 8 * 1024 * 1024;"))
        #expect(Wire.maxBody == 8 * 1024 * 1024)
    }

    @Test func noAnswerIsSaidInTheCoresWords() {
        // Offline is said only when no connection was made, so nothing of the request was sent.
        let unsent: [URLError.Code] = [
            .notConnectedToInternet, .cannotFindHost, .dnsLookupFailed, .cannotConnectToHost, .dataNotAllowed,
            .internationalRoamingOff, .callIsActive,
        ]
        for code in unsent {
            #expect(HTTPTransport.failure(URLError(code)) == .offline, "code \(code.rawValue)")
        }
        #expect(HTTPTransport.failure(URLError(.timedOut)) == .timeout)
        // A connection lost in the middle of a load may have carried the
        // request: a payment sent and never answered is not one never made.
        let perhapsSent: [URLError.Code] = [
            .networkConnectionLost, .secureConnectionFailed, .badServerResponse, .cannotParseResponse,
            .serverCertificateUntrusted, .appTransportSecurityRequiresSecureConnection, .unknown,
        ]
        for code in perhapsSent {
            #expect(HTTPTransport.failure(URLError(code)) == .other, "code \(code.rawValue)")
        }
        #expect(HTTPTransport.failure(CocoaError(.fileNoSuchFile)) == .other)
        #expect(String(decoding: CoreEvent.response(.failed(3, .offline)).json, as: UTF8.self) == #"{"error":"offline","request":3,"t":"response"}"#)
    }

    @Test func multipartPartsGoInOrder() throws {
        let parts: [FetchRequest.Part] = [
            .text(name: "note", value: "Wrap the dupattas"),
            .file(name: "photo", token: "pick-1", filename: "corner.jpg", type: "image/jpeg"),
        ]
        let made = try #require(Multipart.body(parts, boundary: "b", files: { $0 == "pick-1" ? Data("jpeg-bytes".utf8) : nil }))
        let body = String(decoding: made, as: UTF8.self)
        #expect(body.hasPrefix("--b\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nWrap the dupattas\r\n--b\r\n"))
        #expect(body.contains("filename=\"corner.jpg\"\r\nContent-Type: image/jpeg\r\n\r\njpeg-bytes\r\n"))
        #expect(body.hasSuffix("--b--\r\n"))
        // Parts with no file need none.
        #expect(Multipart.body([parts[0]], boundary: "b") != nil)
    }

    @Test func aFileWhoseBytesAreGoneIsNeverSentEmptyUnderItsName() async throws {
        let host = "files-gone.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: [:], body: "{}"))
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-files-gone-\(UUID().uuidString)")
        let store = FileStore(folder: folder.appendingPathComponent("kept"))
        defer { try? FileManager.default.removeItem(at: folder) }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let picked = folder.appendingPathComponent("contract.pdf")
        try Data("%PDF-1.7 signed by Harleen Sandhu, Ludhiana".utf8).write(to: picked)
        let file = try #require(store.keep(picked))
        let parts: [FetchRequest.Part] = [
            .text(name: "note", value: "Signed copy"),
            .file(name: "contract", token: file.token, filename: file.name, type: file.type),
        ]
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration, files: store)
        let request = FetchRequest(
            request: 12, method: "post", url: "https://\(host)/orders/1/documents", body: .multipart(parts)
        )
        // While the bytes are held, the request is one that goes.
        #expect(transport.urlRequest(for: request)?.httpBody?.isEmpty == false)

        // The files were let go (the person signed out in another window).
        store.clear()
        #expect(Multipart.body(parts, boundary: "b", files: store.data) == nil)
        #expect(transport.urlRequest(for: request) == nil)
        #expect(await fetched(transport, request) == .failed(12, .other))
        #expect(StubOrigin.seen(host).isEmpty)
    }

    @Test func aPictureIsAskedForWithTheSessionsCookiesFromTheOneJar() async throws {
        let host = "pictures.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: ["Content-Type": "image/png"], body: "png-bytes"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        jar.merge([CookieRecord(name: "session", value: "s3cret", domain: host)])
        let pictures = PictureLoader(jar: jar, configuration: StubOrigin.configuration)
        let data = await pictures.load(URL(string: "https://\(host)/storage/products/18.png")!)
        #expect(data == Data("png-bytes".utf8))
        #expect(StubOrigin.seen(host).last?.value(forHTTPHeaderField: "Cookie") == "session=s3cret")
        // Another site's picture goes without them.
        #expect(pictures.request(URL(string: "https://cdn.elsewhere.example/a.png")!).value(forHTTPHeaderField: "Cookie") == nil)
    }

    @Test func aPickedFileIsSentWithItsBytes() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-files-test-\(UUID().uuidString)")
        let store = FileStore(folder: folder.appendingPathComponent("kept"))
        defer { try? FileManager.default.removeItem(at: folder) }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let picked = folder.appendingPathComponent("packing-list.csv")
        try Data("sku,qty\nKIT-307-000,5\n".utf8).write(to: picked)
        let file = try #require(store.keep(picked))
        #expect(file.name == "packing-list.csv")
        #expect(file.type == "text/csv")
        #expect(file.size == 22)
        let made = try #require(Multipart.body(
            [.file(name: "list", token: file.token, filename: file.name, type: file.type)], boundary: "b", files: store.data
        ))
        let body = String(decoding: made, as: UTF8.self)
        #expect(body.contains("Content-Type: text/csv\r\n\r\nsku,qty\nKIT-307-000,5\n\r\n--b--"))
        store.clear()
        #expect(store.data(file.token) == nil)
    }
}

@Test func aStreamsLineIsCutAtItsBreakAndOfALongOneOnlyTheStartIsHeld() {
    var cutter = LineCutter(most: 8)
    var lines: [String] = []
    for byte in "event: a\n\r\ndata: 0123456789abcdef".utf8 {
        if let line = cutter.take(byte) { lines.append(line) }
    }
    // A line of exactly the most that is held, and a blank one that ends an event.
    #expect(lines == ["event: a", ""])
    // A line that goes on past it is given by its start, once it ends.
    for byte in "ghij\nok\r\n".utf8 {
        if let line = cutter.take(byte) { lines.append(line) }
    }
    #expect(lines == ["event: a", "", "data: 01", "ok"])
    // However long a line with no end is, no more of it is held.
    var endless = LineCutter()
    var given = 0
    for _ in 0..<(LineCutter.most * 3) where endless.take(0x78) != nil { given += 1 }
    #expect(given == 0)
    #expect(endless.take(0x0A)?.utf8.count == LineCutter.most)
}

@MainActor
struct StreamTests {
    /// What a stream said, in order.
    @MainActor
    final class Heard {
        var states: [CoreEvent.StreamState] = []
    }

    @Test func aStreamIsReadToItsEndWhateverTheLengthOfItsLines() async {
        let host = "streams-long.shop.example"
        let long = String(repeating: "x", count: LineCutter.most * 3)
        StubOrigin.answer(host, .init(
            status: 200, headers: ["Content-Type": "text/event-stream"],
            body: "event: orders\ndata: \(long)\n\n: still here\nevent: stats\r\ndata: {}\r\n\r\n"
        ))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let heard = Heard()
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            transport.open(stream: 4, url: "https://\(host)/broadcasting/stream?topics=orders,stats", headers: []) { state in
                heard.states.append(state)
                if case .closed = state { done.resume() }
            }
        }
        #expect(heard.states == [.open, .event(name: "orders"), .event(name: "stats"), .closed(status: 200)])
    }

    @Test func aStreamTheCoreClosedSaysNothingMore() async throws {
        let host = "streams-closed.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: [:], body: "event: orders\ndata: {}\n\n"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let heard = Heard()
        transport.open(stream: 5, url: "https://\(host)/broadcasting/stream", headers: []) { heard.states.append($0) }
        transport.close(stream: 5)
        try await Task.sleep(for: .milliseconds(300))
        #expect(heard.states.isEmpty)
    }
}

@Test func anEventStreamSaysEachEventsNameAndNeverItsData() {
    var reader = EventStreamReader()
    var events: [String?] = []
    for line in [": hello", "event: orders", "data: {\"id\":1}", "", "data: x", "", "event: stats", "", "retry: 10"] {
        if let event = reader.read(line) { events.append(event) }
    }
    #expect(events == ["orders", nil])
}

@MainActor
struct CookieTests {
    @Test func aCookieGoesOnlyWhereItsDomainPathAndSecurityAllow() {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([
            CookieRecord(name: "session", value: "a", domain: "shop.example"),
            CookieRecord(name: "wide", value: "b", domain: ".shop.example", path: "/orders"),
            CookieRecord(name: "plain", value: "c", domain: "127.0.0.1", secure: true),
        ])
        #expect(jar.header(for: URL(string: "https://shop.example/")!) == "session=a")
        #expect(jar.header(for: URL(string: "https://shop.example/orders/1")!) == "wide=b; session=a")
        #expect(jar.header(for: URL(string: "https://eu.shop.example/orders")!) == "wide=b")
        #expect(jar.header(for: URL(string: "http://shop.example/")!) == nil)
        #expect(jar.header(for: URL(string: "https://other.example/")!) == nil)
        // This machine is a secure context, as in a browser.
        #expect(jar.header(for: URL(string: "http://127.0.0.1:8095/")!) == "plain=c")
        #expect(jar.hosts == ["shop.example", "127.0.0.1"])
    }

    @Test func anExpiredCookieIsDropped() {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        jar.merge([CookieRecord(name: "session", value: "a", domain: "shop.example")])
        jar.merge([CookieRecord(name: "session", value: "", domain: "shop.example", expires: Date(timeIntervalSince1970: 0))])
        #expect(jar.header(for: URL(string: "https://shop.example/")!) == nil)
    }

    @Test func theXSRFTokenIsTheCookiesValuePercentDecoded() {
        let jar = CookieJar(origin: "https://shop.example", persistent: false)
        #expect(jar.xsrfToken(for: URL(string: "https://shop.example/")!) == nil)
        jar.merge([CookieRecord(name: "XSRF-TOKEN", value: "eyJpdiI6%3D%3D", domain: "shop.example", httpOnly: false)])
        #expect(jar.xsrfToken(for: URL(string: "https://shop.example/orders")!) == "eyJpdiI6==")
    }
}

/// The core keeps a page only when its `cache-control` does not say `no-store`.
@Test func anAnswersCacheControlGoesToTheCoreWithItsOwnHeadersAndNoOther() throws {
    let orders = try #require(URL(string: "https://shop.example/orders"))
    let http = try #require(HTTPURLResponse(
        url: orders, statusCode: 200, httpVersion: "HTTP/1.1",
        headerFields: [
            "Cache-Control": "private, no-store", "X-Navree-Keep": "1", "Set-Cookie": "session=s3cret; Path=/",
            "Content-Type": "application/vnd.navree.native+json",
        ]
    ))
    let response = HTTPTransport.response(1, http, Data("{}".utf8))
    #expect(response.headers == [["cache-control", "private, no-store"], ["x-navree-keep", "1"]])
}
