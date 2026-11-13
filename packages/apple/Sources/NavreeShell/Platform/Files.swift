import Foundation
import NavreeRuntime
import UniformTypeIdentifiers

/// The files a person picked, kept by the app under tokens until a form sends
/// them: the core holds only each file's token, name, type and size, and a
/// multipart post reads the bytes from here. Kept in the app's own temporary
/// folder; emptied when the person signs out.
@MainActor
public final class FileStore {
    public static let shared = FileStore()

    private let folder: URL
    private var kept: [String: URL] = [:]

    public init(folder: URL = FileManager.default.temporaryDirectory.appendingPathComponent("navree-files", isDirectory: true)) {
        self.folder = folder
    }

    /// Copies the file at `url` and keeps it under a new token; nothing when it can't be read.
    public func keep(_ url: URL) -> FileRef? {
        let token = UUID().uuidString.lowercased()
        let copy = folder.appendingPathComponent(token)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: url, to: copy)
        } catch {
            return nil
        }
        kept[token] = copy
        let size = (try? copy.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        return FileRef(token: token, name: url.lastPathComponent, type: Self.type(of: url), size: size)
    }

    /// The bytes kept under `token`.
    public func data(_ token: String) -> Data? {
        kept[token].flatMap { try? Data(contentsOf: $0) }
    }

    /// Lets every kept file go.
    public func clear() {
        kept.removeAll()
        try? FileManager.default.removeItem(at: folder)
    }

    /// A file's media type, from its name's extension.
    nonisolated static func type(of url: URL) -> String {
        UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
    }
}
