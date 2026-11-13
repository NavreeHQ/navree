import Foundation
import UniformTypeIdentifiers

/// What the system's file browser offers for a file field.
public enum FileRules {
    /// The types a field's `accept` names, the web's way (`image/*`,
    /// `application/pdf`, `.csv`), as the system's types; any file when it
    /// names none this system knows.
    public static func types(_ accept: String?) -> [UTType] {
        let names = (accept ?? "").split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
        let types = names.compactMap { name -> UTType? in
            if name.hasPrefix(".") { return UTType(filenameExtension: String(name.dropFirst())) }
            if name.hasSuffix("/*") {
                switch name.dropLast(2) {
                case "image": return .image
                case "video": return .movie
                case "audio": return .audio
                case "text": return .text
                default: return nil
                }
            }
            // A type the system only makes up for the name is no type it knows.
            return UTType(mimeType: name).flatMap { $0.isDynamic ? nil : $0 }
        }
        return types.isEmpty ? [.item] : types
    }
}
