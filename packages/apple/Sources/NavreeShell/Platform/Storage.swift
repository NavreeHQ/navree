import CryptoKit
import Foundation

/// Files the client keeps for the runtime core: in a folder of the app's own,
/// out of every backup and device transfer, each written with complete data
/// protection (unreadable while the device is locked).
///
/// The core's key-value cache lives in one (keys are opaque, each a file named
/// by its SHA-256); the core empties it when the person changes or signs out.
/// What is kept of each window for a launch the system may end lives in
/// another, a file for each window. Both are the session's, one place each for
/// every window: a session that ends takes all of both with it.
public final class KeptFiles: Sendable {
    public let folder: URL

    /// A folder named `name` in one of the app's own folders: its caches for
    /// what may be lost, its Application Support for what may not.
    public convenience init(_ name: String, in place: FileManager.SearchPathDirectory = .applicationSupportDirectory) {
        let base = FileManager.default.urls(for: place, in: .userDomainMask).first ?? FileManager.default.temporaryDirectory
        self.init(folder: base.appendingPathComponent(name, isDirectory: true))
    }

    public init(folder: URL) {
        self.folder = folder
    }

    public func read(_ key: String) -> String? {
        guard let data = try? Data(contentsOf: file(key)) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    public func readData(_ key: String) -> Data? {
        try? Data(contentsOf: file(key))
    }

    /// Writes `value` under `key`; `nil` deletes it.
    public func write(_ key: String, _ value: String?) {
        writeData(key, value.map { Data($0.utf8) })
    }

    public func writeData(_ key: String, _ value: Data?) {
        let file = file(key)
        guard let value else {
            try? FileManager.default.removeItem(at: file)
            return
        }
        do {
            try prepareFolder()
            try value.write(to: file, options: [.atomic, .completeFileProtection])
        } catch {
            try? FileManager.default.removeItem(at: file)
        }
    }

    /// Deletes everything kept here. `false` when something of it is still
    /// on the device afterwards.
    @discardableResult
    public func clear() -> Bool {
        do {
            try FileManager.default.removeItem(at: folder)
            return true
        } catch {
            return !FileManager.default.fileExists(atPath: folder.path)
        }
    }

    /// Deletes every folder that stands in this one. What is kept here is
    /// files, each named by its key's digest: a folder among them is not this
    /// store's, and nothing reads it.
    public func clearFolders() {
        for item in entries([.isDirectoryKey]) where (try? item.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true {
            try? FileManager.default.removeItem(at: item)
        }
    }

    /// Deletes each file last written before `date`.
    public func clear(writtenBefore date: Date) {
        for item in entries([.contentModificationDateKey]) {
            let written = (try? item.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
            if let written, written < date { try? FileManager.default.removeItem(at: item) }
        }
    }

    /// What stands in the folder now.
    private func entries(_ keys: [URLResourceKey]) -> [URL] {
        (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys)) ?? []
    }

    /// The file a key is kept in.
    func file(_ key: String) -> URL {
        let digest = SHA256.hash(data: Data(key.utf8)).map { String(format: "%02x", $0) }.joined()
        return folder.appendingPathComponent(digest)
    }

    private func prepareFolder() throws {
        guard !FileManager.default.fileExists(atPath: folder.path) else { return }
        try FileManager.default.createDirectory(
            at: folder, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.complete]
        )
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        var folder = folder
        try folder.setResourceValues(values)
    }

    /// Whether the folder is left out of backups.
    var isExcludedFromBackup: Bool {
        (try? folder.resourceValues(forKeys: [.isExcludedFromBackupKey]))?.isExcludedFromBackup ?? false
    }
}
