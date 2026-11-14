import CryptoKit
import Foundation

/// Files the client keeps for the runtime core: in a folder of the app's own,
/// out of every backup and device transfer, each written with complete data
/// protection (unreadable while the device is locked).
///
/// The core's key-value cache lives in one (keys are opaque, each a file named
/// by its SHA-256); the core empties it when the person changes or signs out.
/// The snapshot of a launch lives in another.
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

    /// A folder of its own inside this one, named for `name` (its letters,
    /// digits and `-` kept, anything else a `-`): what a scene keeps apart
    /// from the others. Emptying it leaves the others as they are.
    public func within(_ name: String) -> KeptFiles {
        let safe = String(name.unicodeScalars.map { scalar -> Character in
            scalar.isASCII && (CharacterSet.alphanumerics.contains(scalar) || scalar == "-") ? Character(scalar) : "-"
        })
        return KeptFiles(folder: folder.appendingPathComponent("scene-" + safe, isDirectory: true))
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

    /// Deletes the files kept in this folder itself, leaving the folders
    /// within it: what a build that kept one store for every scene left.
    public func clearLoose() {
        let items = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.isDirectoryKey])) ?? []
        for item in items where (try? item.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory != true {
            try? FileManager.default.removeItem(at: item)
        }
    }

    /// Deletes everything kept here.
    public func clear() {
        try? FileManager.default.removeItem(at: folder)
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
