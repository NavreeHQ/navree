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

    @Test func noAnswerIsSaidInTheCoresWords() {
        #expect(HTTPTransport.failure(URLError(.notConnectedToInternet)) == .offline)
        #expect(HTTPTransport.failure(URLError(.cannotFindHost)) == .offline)
        #expect(HTTPTransport.failure(URLError(.timedOut)) == .timeout)
        #expect(HTTPTransport.failure(URLError(.badServerResponse)) == .other)
        #expect(String(decoding: CoreEvent.response(.failed(3, .offline)).json, as: UTF8.self) == #"{"error":"offline","request":3,"t":"response"}"#)
    }

    @Test func multipartPartsGoInOrder() {
        let body = String(decoding: Multipart.body(
            [.text(name: "note", value: "Wrap the dupattas"), .file(name: "photo", token: "pick-1", filename: "corner.jpg", type: "image/jpeg")],
            boundary: "b"
        ), as: UTF8.self)
        #expect(body.hasPrefix("--b\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nWrap the dupattas\r\n--b\r\n"))
        #expect(body.contains("filename=\"corner.jpg\"\r\nContent-Type: image/jpeg"))
        #expect(body.hasSuffix("--b--\r\n"))
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
        let body = String(decoding: Multipart.body(
            [.file(name: "list", token: file.token, filename: file.name, type: file.type)], boundary: "b", files: store.data
        ), as: UTF8.self)
        #expect(body.contains("Content-Type: text/csv\r\n\r\nsku,qty\nKIT-307-000,5\n\r\n--b--"))
        store.clear()
        #expect(store.data(file.token) == nil)
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
