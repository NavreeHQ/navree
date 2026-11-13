import Foundation
import NavreeRuntime

/// Answers recorded from a server, kept in a folder outside the app, served
/// in place of the network: for drawing an app's screens with no server at
/// all. Nothing of such a folder is part of the app.
///
/// The folder holds `manifest.tsv` (tab-separated, a header row, the columns
/// `file`, `status` and `url` at least; other columns are passed over) and
/// the files it names. A request is answered by the rows of its address, by
/// path and query: several rows of one address are served in order, and then
/// the last one again.
@MainActor
public final class AnswerFolder: Transport {
    public struct Row: Equatable, Sendable {
        public var file: String
        public var status: Int
        public var url: String
    }

    public let folder: URL
    public let rows: [Row]
    private var served: [String: Int] = [:]

    public var jar: CookieJar? { nil }

    /// The folder's answers, or `nil` when it has no manifest that reads.
    public convenience init?(folder: URL) {
        guard let rows = Self.manifest(folder) else { return nil }
        self.init(folder: folder, rows: rows)
    }

    init(folder: URL, rows: [Row]) {
        self.folder = folder
        self.rows = rows
    }

    /// No answers: every request is answered as not recorded.
    public static func empty() -> AnswerFolder {
        AnswerFolder(folder: URL(fileURLWithPath: "/"), rows: [])
    }

    /// The origin the answers were recorded from, when the manifest names
    /// one: the first row whose address is absolute.
    public nonisolated static func origin(of folder: URL) -> String? {
        for row in manifest(folder) ?? [] {
            guard let url = URL(string: row.url), let scheme = url.scheme, let host = url.host else { continue }
            return "\(scheme)://\(host)" + (url.port.map { ":\($0)" } ?? "")
        }
        return nil
    }

    nonisolated static func manifest(_ folder: URL) -> [Row]? {
        guard let text = try? String(contentsOf: folder.appendingPathComponent("manifest.tsv"), encoding: .utf8) else {
            return nil
        }
        return rows(text)
    }

    /// The rows of a manifest; `nil` when it lacks a column it needs.
    public nonisolated static func rows(_ text: String) -> [Row]? {
        let lines = text.split(whereSeparator: \.isNewline).map { $0.split(separator: "\t", omittingEmptySubsequences: false).map(String.init) }
        guard let header = lines.first?.map({ $0.trimmingCharacters(in: .whitespaces).lowercased() }),
            let file = header.firstIndex(of: "file"), let status = header.firstIndex(of: "status"),
            let url = header.firstIndex(of: "url")
        else { return nil }
        return lines.dropFirst().compactMap { cells in
            guard cells.count > max(file, status, url), !cells[file].isEmpty else { return nil }
            return Row(
                file: cells[file].trimmingCharacters(in: .whitespaces), status: Int(cells[status].trimmingCharacters(in: .whitespaces)) ?? 200,
                url: cells[url].trimmingCharacters(in: .whitespaces)
            )
        }
    }

    /// The address a request or a row names, by its path and query.
    nonisolated static func address(_ url: String) -> String {
        guard let components = URLComponents(string: url), components.scheme != nil || url.hasPrefix("/") else { return url }
        let path = components.percentEncodedPath.isEmpty ? "/" : components.percentEncodedPath
        return path + (components.percentEncodedQuery.map { "?" + $0 } ?? "")
    }

    /// The row that answers the next request for `url`.
    public func next(for url: String) -> Row? {
        let address = Self.address(url)
        let matching = rows.filter { Self.address($0.url) == address }
        guard !matching.isEmpty else { return nil }
        let count = served[address, default: 0]
        served[address] = count + 1
        return matching[min(count, matching.count - 1)]
    }

    public func fetch(_ request: FetchRequest, answer: @escaping @MainActor (Response) -> Void) {
        guard let row = next(for: request.url) else {
            return answer(Response(request: request.request, status: 404, contentType: "text/plain", body: "Not recorded"))
        }
        let body = (try? String(contentsOf: folder.appendingPathComponent(row.file), encoding: .utf8)) ?? ""
        let response = Response(
            request: request.request, status: row.status, contentType: "application/vnd.navree.native+json", body: body
        )
        // Answered after the event that asked, as the network would.
        Task { @MainActor in answer(response) }
    }

    public func cancel(_ request: Int) {}

    /// A recorded folder has no live streams: they stay quiet.
    public func open(
        stream: Int, url: String, headers: [(String, String)], report: @escaping @MainActor (CoreEvent.StreamState) -> Void
    ) {}

    public func close(stream: Int) {}
}
