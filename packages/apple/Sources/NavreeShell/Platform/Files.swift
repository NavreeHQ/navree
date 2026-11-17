import Foundation
import NavreeRuntime
import UniformTypeIdentifiers

/// The files a person picked, kept by the app under tokens until a form sends
/// them: the core holds only each file's token, name, type and size, and a
/// multipart post reads the bytes from here. Kept in the app's own temporary
/// folder; emptied when the person signs out. A photo is kept without the
/// place it was taken, unless its field asks for the place.
@MainActor
public final class FileStore {
    public static let shared = FileStore()

    private let folder: URL
    private var kept: [String: URL] = [:]

    public init(folder: URL = FileManager.default.temporaryDirectory.appendingPathComponent("navree-files", isDirectory: true)) {
        self.folder = folder
    }

    /// Copies the file at `url` and keeps it under a new token; nothing when
    /// it can't be read. A photo is kept without the place it was taken
    /// unless the field it was picked for says to keep it (`keepLocation`,
    /// the field's `keep_location`); one whose place cannot be taken out is
    /// not kept, so it is never sent as it was (``PhotoPlace``).
    public func keep(_ url: URL, keepLocation: Bool = false) -> FileRef? {
        let token = UUID().uuidString.lowercased()
        let copy = folder.appendingPathComponent(token)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        } catch {
            return nil
        }
        guard PhotoPlace.copy(url, to: copy, keepingPlace: keepLocation) else { return nil }
        kept[token] = copy
        let size = (try? copy.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        return FileRef(token: token, name: url.lastPathComponent, type: Self.type(of: url), size: size)
    }

    /// The bytes kept under `token`.
    public func data(_ token: String) -> Data? {
        kept[token].flatMap { try? Data(contentsOf: $0) }
    }

    /// Lets every kept file go. `false` when one is still on the device afterwards.
    @discardableResult
    public func clear() -> Bool {
        kept.removeAll()
        do {
            try FileManager.default.removeItem(at: folder)
            return true
        } catch {
            return !FileManager.default.fileExists(atPath: folder.path)
        }
    }

    /// A file's media type, from its name's extension.
    nonisolated static func type(of url: URL) -> String {
        UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
    }
}
