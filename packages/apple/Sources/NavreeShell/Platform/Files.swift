import Foundation
import NavreeRuntime
import Observation
import UniformTypeIdentifiers

/// Why a file a person picked was not added to its field.
public enum FileRefusal: Error, Sendable, Equatable {
    /// It could not be read, or its copy could not be written.
    case unreadable
    /// A picture by its name that does not read as one: whether it says
    /// where it was taken cannot be told.
    case placeUnchecked
    /// A picture in a format from which details that could say where it was
    /// taken cannot be removed.
    case placeStays
    /// A film that could not be exported without the place it was made.
    case filmPlaceStays
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
/// folder; emptied when the person signs out. A photo or a film is kept
/// without what could say where it was made, unless its field asks for the
/// place (`keep_location`).
@MainActor
public final class FileStore {
    public static let shared = FileStore()

    private let folder: URL
    private var kept: [String: URL] = [:]
    private var generation = 0
    private var inFlight = 0
    /// Optional hook used by tests to observe off-main-actor preparation.
    var workObserver: (@Sendable () async -> Void)?

    public init(
        folder: URL = FileManager.default.temporaryDirectory.appendingPathComponent("navree-files", isDirectory: true)
    ) {
        self.folder = folder
    }

    /// Synchronously copies the file at `url` and keeps it under a new token,
    /// or says why it was not added. Films require asynchronous export unless
    /// `keepLocation` is `true`, so synchronous callers refuse unkept films
    /// rather than copying them with their place.
    public func take(_ url: URL, keepLocation: Bool = false) -> PickedFile {
        if !keepLocation && FilmPlace.isFilm(url) {
            return .refused(.filmPlaceStays)
        }
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
        let size = Self.byteCount(of: copy)
        return .kept(FileRef(token: token, name: url.lastPathComponent, type: Self.type(of: url), size: size))
    }

    /// The same, for one who only asks whether the file was kept.
    public func keep(_ url: URL, keepLocation: Bool = false) -> FileRef? {
        if case .kept(let file) = take(url, keepLocation: keepLocation) { return file }
        return nil
    }

    /// Prepares `url` off the main actor — rewriting a photo through
    /// ``PhotoPlace`` or exporting a film through ``FilmPlace`` unless
    /// `keepLocation` is `true` — and registers the clean copy under a token.
    func takeAsync(
        _ url: URL,
        keepLocation: Bool = false,
        propertyReading: PhotoPlace.PropertyReading = .system,
        rewriter: @escaping PhotoPlace.Rewriter = PhotoPlace.defaultRewriter,
        filmExporter: @escaping FilmPlace.Exporter = FilmPlace.systemExport
    ) async -> PickedFile {
        let startedIn = generation
        let folder = self.folder
        let observer = self.workObserver
        let token = UUID().uuidString.lowercased()
        let copy = folder.appendingPathComponent(token)
        let name = url.lastPathComponent

        inFlight += 1
        let prepared = await Task.detached(priority: .userInitiated) { () -> Result<Int, FileRefusal> in
            if let observer { await observer() }
            if Task.isCancelled { return .failure(.unreadable) }
            do {
                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            } catch {
                return .failure(.unreadable)
            }
            return await Self.prepareFile(
                at: url, to: copy, keepLocation: keepLocation,
                propertyReading: propertyReading, rewriter: rewriter, filmExporter: filmExporter
            )
        }.value
        inFlight -= 1

        guard generation == startedIn, !Task.isCancelled else {
            try? FileManager.default.removeItem(at: copy)
            removeFolderIfEmpty()
            return .refused(.unreadable)
        }

        switch prepared {
        case .success(let size):
            kept[token] = copy
            return .kept(FileRef(token: token, name: name, type: Self.type(of: url), size: size))
        case .failure(let refusal):
            try? FileManager.default.removeItem(at: copy)
            removeFolderIfEmpty()
            return .refused(refusal)
        }
    }

    /// Asynchronous counterpart of ``keep(_:keepLocation:)``.
    func keepAsync(
        _ url: URL,
        keepLocation: Bool = false,
        filmExporter: @escaping FilmPlace.Exporter = FilmPlace.systemExport
    ) async -> FileRef? {
        if case .kept(let file) = await takeAsync(url, keepLocation: keepLocation, filmExporter: filmExporter) {
            return file
        }
        return nil
    }

    /// Prepares files chosen for a web page's `<input type="file">` off the
    /// main actor, always stripping photo and film location (`keepLocation`
    /// is never set on a web input), and returns the clean local file URLs
    /// together with the words for any files that were refused.
    func prepareForWeb(
        _ urls: [URL],
        multiple: Bool = true,
        filmExporter: @escaping FilmPlace.Exporter = FilmPlace.systemExport
    ) async -> (kept: [URL], refused: [String]) {
        let candidates = multiple ? urls : Array(urls.prefix(1))
        let startedIn = generation
        let folder = self.folder
        let observer = self.workObserver
        var preparedURLs: [URL] = []
        var createdFolders: [URL] = []
        var refused: [String] = []

        inFlight += 1
        for url in candidates {
            if Task.isCancelled { break }
            let lent = url.startAccessingSecurityScopedResource()
            let slot = folder.appendingPathComponent("web-\(UUID().uuidString.lowercased())", isDirectory: true)
            let safeName = Self.safeName(url.lastPathComponent)
            let target = slot.appendingPathComponent(safeName)
            let outcome = await Task.detached(priority: .userInitiated) { () -> Result<Void, FileRefusal> in
                if let observer { await observer() }
                if Task.isCancelled { return .failure(.unreadable) }
                do {
                    try FileManager.default.createDirectory(at: slot, withIntermediateDirectories: true)
                } catch {
                    return .failure(.unreadable)
                }
                let result = await Self.prepareFile(
                    at: url, to: target, keepLocation: false,
                    propertyReading: .system,
                    rewriter: PhotoPlace.defaultRewriter,
                    filmExporter: filmExporter
                )
                return result.map { _ in () }
            }.value
            if lent { url.stopAccessingSecurityScopedResource() }

            switch outcome {
            case .success:
                createdFolders.append(slot)
                preparedURLs.append(target)
            case .failure(let why):
                try? FileManager.default.removeItem(at: slot)
                refused.append(Words.refused(url.lastPathComponent, why))
            }
        }
        inFlight -= 1

        guard generation == startedIn, !Task.isCancelled else {
            for slot in createdFolders { try? FileManager.default.removeItem(at: slot) }
            removeFolderIfEmpty()
            return ([], [])
        }
        if preparedURLs.isEmpty {
            removeFolderIfEmpty()
        }
        return (preparedURLs, refused)
    }

    private func removeFolderIfEmpty() {
        guard inFlight == 0, kept.isEmpty else { return }
        if (try? FileManager.default.contentsOfDirectory(atPath: folder.path))?.isEmpty == true {
            try? FileManager.default.removeItem(at: folder)
        }
    }

    private nonisolated static func prepareFile(
        at source: URL,
        to copy: URL,
        keepLocation: Bool,
        propertyReading: PhotoPlace.PropertyReading,
        rewriter: PhotoPlace.Rewriter,
        filmExporter: FilmPlace.Exporter
    ) async -> Result<Int, FileRefusal> {
        let ext = source.pathExtension
        if !keepLocation && FilmPlace.isFilm(source, originalExtension: ext) {
            switch await FilmPlace.copy(
                source, originalExtension: ext, to: copy, keepingPlace: false, exporter: filmExporter
            ) {
            case .copied:
                return .success(byteCount(of: copy))
            case .unreadable:
                return .failure(.unreadable)
            case .placeStays:
                return .failure(.filmPlaceStays)
            }
        }
        switch PhotoPlace.copy(
            source, to: copy, keepingPlace: keepLocation, propertyReading: propertyReading, rewriter: rewriter
        ) {
        case .copied:
            return .success(byteCount(of: copy))
        case .unreadable:
            return .failure(.unreadable)
        case .unchecked:
            return .failure(.placeUnchecked)
        case .placeStays:
            return .failure(.placeStays)
        }
    }

    /// Removes specific kept tokens when a pick is superseded or its screen
    /// disappeared before preparation finished.
    func discard(_ tokens: [String]) {
        for token in tokens {
            if let url = kept.removeValue(forKey: token) {
                try? FileManager.default.removeItem(at: url)
            }
        }
    }

    /// The bytes kept under `token`.
    public func data(_ token: String) -> Data? {
        kept[token].flatMap { try? Data(contentsOf: $0) }
    }

    /// Lets every kept file go. `false` when one is still on the device afterwards.
    @discardableResult
    public func clear() -> Bool {
        generation &+= 1
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

    private nonisolated static func byteCount(of url: URL) -> Int {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            let number = attributes[.size] as? NSNumber
        else { return 0 }
        return number.intValue
    }

    private nonisolated static func safeName(_ raw: String) -> String {
        let cleaned = raw.unicodeScalars.filter { scalar in
            scalar.value >= 0x20 && scalar != "/" && scalar != "\\" && scalar != ":"
        }
        let text = String(String.UnicodeScalarView(cleaned)).trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty || text == "." || text == ".." { return "file" }
        return text
    }
}

/// What the app says, at a file field or over a web screen, of the files a
/// person picked that were not added: each by its name, with why; and whether
/// a field is still preparing picked files off the main actor.
@MainActor
@Observable
final class FileNotes {
    struct FieldKey: Hashable, Sendable {
        let screen: ScreenID
        let form: String
        let field: String
    }

    private var said: [FieldKey: [String]] = [:]
    private var webSaid: [ScreenID: [String]] = [:]
    private var active: [FieldKey: Int] = [:]
    @ObservationIgnored private var tasks: [FieldKey: Task<Void, Never>] = [:]
    @ObservationIgnored private var nextSerial = 0

    /// Says `words` at a field, in place of what was said there before; no
    /// words says nothing there any more.
    func say(_ words: [String], screen: ScreenID, form: String, field: String) {
        let key = FieldKey(screen: screen, form: form, field: field)
        said[key] = words.isEmpty ? nil : words
    }

    /// What is said at a field now.
    func words(screen: ScreenID, form: String, field: String) -> [String] {
        said[FieldKey(screen: screen, form: form, field: field)] ?? []
    }

    /// Says `words` for a web screen's file picker; empty clears it.
    func sayOnWeb(_ words: [String], screen: ScreenID) {
        webSaid[screen] = words.isEmpty ? nil : words
    }

    /// What is said for a web screen's file picker now.
    func webWords(screen: ScreenID) -> [String] {
        webSaid[screen] ?? []
    }

    /// Whether `field` is currently rewriting or exporting picked files.
    func isBusy(screen: ScreenID, form: String, field: String) -> Bool {
        active[FieldKey(screen: screen, form: form, field: field)] != nil
    }

    /// Starts asynchronous file preparation for `field`, cancelling any
    /// earlier unfinished pick for that same field.
    func begin(screen: ScreenID, form: String, field: String, work: @escaping @MainActor (Int) async -> Void) {
        let key = FieldKey(screen: screen, form: form, field: field)
        tasks[key]?.cancel()
        nextSerial &+= 1
        let serial = nextSerial
        active[key] = serial
        said[key] = nil
        tasks[key] = Task { @MainActor [weak self] in
            await work(serial)
            self?.finish(key, serial: serial)
        }
    }

    func isCurrent(_ serial: Int, screen: ScreenID, form: String, field: String) -> Bool {
        active[FieldKey(screen: screen, form: form, field: field)] == serial && !Task.isCancelled
    }

    private func finish(_ key: FieldKey, serial: Int) {
        guard active[key] == serial else { return }
        active.removeValue(forKey: key)
        tasks.removeValue(forKey: key)
    }

    /// Waits for all in-flight field preparations to finish.
    func settled() async {
        while !tasks.isEmpty {
            let pending = Array(tasks.values)
            for task in pending {
                await task.value
            }
        }
    }

    /// Says nothing anywhere any more, and cancels any in-flight preparation:
    /// the words and files belong to the person whose session just ended.
    func clear() {
        for task in tasks.values { task.cancel() }
        tasks.removeAll()
        active.removeAll()
        said.removeAll()
        webSaid.removeAll()
    }
}
