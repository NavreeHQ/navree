import Foundation

/// The store demo's answers, kept in the app, served to the app's own
/// requests in place of a server. The answers to pages were recorded from the
/// store demo; the answers to posts are written in the shapes it answers with.
nonisolated final class RecordedAnswers: URLProtocol, @unchecked Sendable {
    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.host == URL(string: DemoConfiguration.recordedOrigin)?.host
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        guard let url = request.url else { return }
        let body = Self.body(of: request)
        let (status, name) = Self.answer(method: request.httpMethod ?? "GET", url: url, body: body)
        let data = name.flatMap(Self.file) ?? Data("Not found".utf8)
        let type = name == nil ? "text/plain" : "application/vnd.navree.native+json"
        let response = HTTPURLResponse(
            url: url, statusCode: status, httpVersion: "HTTP/1.1",
            headerFields: ["Content-Type": type, "Vary": "X-Navree-Native"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}

    /// Which recorded answer a request gets, and its status.
    static func answer(method: String, url: URL, body: [String: String]) -> (Int, String?) {
        let path = url.path
        let query = url.query ?? ""
        let segments = path.split(separator: "/").map(String.init)
        if method == "POST" {
            switch (segments.first ?? "", segments.last ?? "") {
            case ("orders", "refund"):
                let amount = Double((body["amount"] ?? "").replacingOccurrences(of: ",", with: "")) ?? 0
                return amount > 0 ? (200, "post-refund") : (422, "post-refund-refused")
            case ("orders", "resend"): return (200, "post-resend")
            case ("orders", "cancel"): return (200, "post-cancel")
            case ("inventory", "restock"): return (200, "post-restock")
            default: return (404, nil)
            }
        }
        switch segments {
        case ["_navree", "native", "shell"]: return (200, "shell")
        case []: return (200, "index")
        case ["orders"]:
            return (200, query.contains("open") || query.contains("sort=total") ? "orders_open_by_total" : "orders")
        case ["orders", "999999"]: return (404, "orders_999999")
        case let parts where parts.count == 3 && parts[0] == "orders" && parts[2] == "peek": return (200, "orders_1_peek")
        case let parts where parts.count == 3 && parts[0] == "orders" && parts[2] == "steps": return (200, "orders_1_steps")
        case let parts where parts.count == 3 && parts[0] == "orders" && parts[2] == "others": return (200, "orders_1_others")
        case let parts where parts.count == 2 && parts[0] == "orders": return (200, "orders_1")
        case ["inventory"]: return (200, query.contains("page=2") ? "inventory_page_2" : "inventory")
        case let parts where parts.count == 2 && parts[0] == "fragments": return (200, "fragments_\(parts[1])")
        default: return (404, nil)
        }
    }

    /// The markup of a page the web view shows, for an address.
    static func page(for address: String) -> String? {
        let path = address.split(separator: "?").first.map(String.init) ?? "/"
        guard path == "/", let url = resource("index.embed", "html"),
            var html = try? String(contentsOf: url, encoding: .utf8)
        else { return nil }
        if let css = resource("navree-ui", "css").flatMap({ try? String(contentsOf: $0, encoding: .utf8) }) {
            html = html.replacingOccurrences(of: "</head>", with: "<style>\(css)</style></head>")
        }
        return html.replacingOccurrences(of: "content=\"dark\"", with: "content=\"light dark\"")
    }

    static func file(_ name: String) -> Data? {
        resource(name, "json").flatMap { try? Data(contentsOf: $0) }
    }

    static func resource(_ name: String, _ ext: String) -> URL? {
        Bundle.main.url(forResource: name, withExtension: ext, subdirectory: "Recorded")
            ?? Bundle.main.url(forResource: name, withExtension: ext)
    }

    /// A post's fields: the body comes as a stream to a URL protocol.
    static func body(of request: URLRequest) -> [String: String] {
        var data = request.httpBody ?? Data()
        if data.isEmpty, let stream = request.httpBodyStream {
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                data.append(buffer, count: count)
            }
            stream.close()
        }
        var fields: [String: String] = [:]
        for pair in String(decoding: data, as: UTF8.self).split(separator: "&") {
            let parts = pair.split(separator: "=", maxSplits: 1).map(String.init)
            let decode = { (text: String) in text.replacingOccurrences(of: "+", with: " ").removingPercentEncoding ?? text }
            fields[decode(parts[0])] = parts.count > 1 ? decode(parts[1]) : ""
        }
        return fields
    }
}
