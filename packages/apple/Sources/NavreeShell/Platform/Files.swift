import Foundation
import NavreeRuntime
import Observation
import UniformTypeIdentifiers

/// Why a file a person picked was not added to its field.
public enum FileRefusal: Sendable, Equatable {
    /// It could not be read, or its copy could not be written.
    case unreadable
    /// A picture by its name that does not read as one: whether it says
    /// where it was taken cannot be told.
    case placeUnchecked
    /// A picture that holds what could say where it was taken, in a kind of
    /// file it cannot be taken out of.
    case placeStays
}

/// What became of a file a person picked for a field.
public enum PickedFile: Sendable, Equatable {
    /// It is kept for the form, under its token.
    case kept(FileRef)
    /// It was not added, and why.
    case refused(FileRefusal)
}

/// The files a person picked, kept by the app under tokens until a form sends
/// them: the core holds only each file's token, name, type and size, and a
/// multipart post reads the bytes from here. Kept in the app's own temporary
/// folder; emptied when the person signs out. A photo is kept without what
/// could say where it was taken, unless its field asks for the place.
@MainActor
public final class FileStore {
    public static let shared = FileStore()

    private let folder: URL
    private var kept: [String: URL] = [:]

    public init(folder: URL = FileManager.default.temporaryDirectory.appendingPathComponent("navree-files", isDirectory: true)) {
        self.folder = folder
    }

    /// Copies the file at `url` and keeps it under a new token, or says why
    /// it was not added. A photo is kept without what could say where it was
    /// taken unless the field it was picked for says to keep the place
    /// (`keepLocation`, the field's `keep_location`); one it cannot be taken
    /// out of is not kept, so it is never sent as it was (``PhotoPlace``).
    public func take(_ url: URL, keepLocation: Bool = false) -> PickedFile {
        let token = UUID().uuidString.lowercased()
        let copy = folder.appendingPathComponent(token)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        } catch {
            return .refused(.unreadable)
        }
        switch PhotoPlace.copy(url, to: copy, keepingPlace: keepLocation) {
        case .copied: break
        case .unreadable: return .refused(.unreadable)
        case .unchecked: return .refused(.placeUnchecked)
        case .placeStays: return .refused(.placeStays)
        }
        kept[token] = copy
        let size = (try? copy.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        return .kept(FileRef(token: token, name: url.lastPathComponent, type: Self.type(of: url), size: size))
    }

    /// The same, for one who only asks whether the file was kept.
    public func keep(_ url: URL, keepLocation: Bool = false) -> FileRef? {
        if case .kept(let file) = take(url, keepLocation: keepLocation) { return file }
        return nil
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

/// What the app says, at a file field, of the files a person picked for it
/// and it did not add: each by its name, with why. The core holds a field's
/// files and knows nothing of one that was never added, so these words are
/// the app's own, kept by the screen, the form and the field, until the
/// person picks for that field again or the session ends.
@MainActor
@Observable
final class FileNotes {
    private var said: [String: [String]] = [:]

    private static func place(_ screen: ScreenID, _ form: String, _ field: String) -> String {
        "\(screen) \(form) \(field)"
    }

    /// Says `words` at a field, in place of what was said there before; no
    /// words says nothing there any more.
    func say(_ words: [String], screen: ScreenID, form: String, field: String) {
        said[Self.place(screen, form, field)] = words.isEmpty ? nil : words
    }

    /// What is said at a field now.
    func words(screen: ScreenID, form: String, field: String) -> [String] {
        said[Self.place(screen, form, field)] ?? []
    }

    /// Says nothing anywhere any more: the words name a person's files.
    func clear() {
        said = [:]
    }
}
