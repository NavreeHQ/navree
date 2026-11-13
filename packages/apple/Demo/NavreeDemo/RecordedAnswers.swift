import Foundation
import NavreeRuntime
import NavreeShell

/// The store demo's answers, kept in the app, given to the runtime core's
/// requests in place of a server. The answers to pages were recorded from the
/// store demo; the answers to posts are written in the shapes it answers with.
@MainActor
final class RecordedStore: Transport {
    var jar: CookieJar? { nil }

    func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        let fields: [String: String]
        if case .form(let text)? = request.body { fields = Self.fields(text) } else { fields = [:] }
        let url = URL(string: request.url)
        let (status, name) = url.map { Self.answer(method: request.method.uppercased(), url: $0, body: fields) } ?? (404, nil)
        let data = name.flatMap(Self.file) ?? Data("Not found".utf8)
        let response = Response(
            request: request.request, status: status,
            contentType: name == nil ? "text/plain" : "application/vnd.navree.native+json",
            body: String(decoding: data, as: UTF8.self)
        )
        // Answered after the event that asked, as the network would.
        Task { @MainActor in answer(response) }
    }

    func cancel(_ request: Int) {}

    /// The recorded store has no live streams: they stay quiet.
    func open(
        stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) {}

    func close(stream: Int) {}

    /// Which recorded answer a request gets, and its status.
    nonisolated static func answer(method: String, url: URL, body: [String: String]) -> (Int, String?) {
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
    ///
    /// The web view asks the network for what the page links to, and the
    /// recorded origin has no server: a stylesheet linked in the head would
    /// hold the page's first paint until its request fails, however long that
    /// takes. So the toolkit's own stylesheets, font and scripts are taken out
    /// and its stylesheets put in the page itself.
    nonisolated static func page(for address: String) -> String? {
        let path = address.split(separator: "?").first.map(String.init) ?? "/"
        guard path == "/", let url = resource("index.embed", "html"),
            var html = try? String(contentsOf: url, encoding: .utf8)
        else { return nil }
        for pattern in [#"<link [^>]*href="/_navree/ui/[^"]*"[^>]*>"#, #"<script [^>]*src="/_navree/ui/[^"]*"[^>]*></script>"#] {
            html = html.replacingOccurrences(of: pattern, with: "", options: .regularExpression)
        }
        let css = ["navree-ui", "charts-styles"]
            .compactMap { resource($0, "css").flatMap { try? String(contentsOf: $0, encoding: .utf8) } }
            .joined(separator: "\n")
        html = html.replacingOccurrences(of: "</head>", with: "<style>\(css)</style></head>")
        // The page follows the system's appearance, as the bars around it do.
        return html
            .replacingOccurrences(of: "content=\"dark\"", with: "content=\"light dark\"")
            .replacingOccurrences(of: "<html ", with: "<html data-mode=\"system\" ")
    }

    nonisolated static func file(_ name: String) -> Data? {
        resource(name, "json").flatMap { try? Data(contentsOf: $0) }
    }

    nonisolated static func resource(_ name: String, _ ext: String) -> URL? {
        Bundle.main.url(forResource: name, withExtension: ext, subdirectory: "Recorded")
            ?? Bundle.main.url(forResource: name, withExtension: ext)
    }

    /// A post's fields, from the form body the core wrote.
    nonisolated static func fields(_ text: String) -> [String: String] {
        var fields: [String: String] = [:]
        for pair in text.split(separator: "&") {
            let parts = pair.split(separator: "=", maxSplits: 1).map(String.init)
            let decode = { (text: String) in text.replacingOccurrences(of: "+", with: " ").removingPercentEncoding ?? text }
            fields[decode(parts[0])] = parts.count > 1 ? decode(parts[1]) : ""
        }
        return fields
    }
}
