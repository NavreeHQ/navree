import Foundation
import ImageIO
import UniformTypeIdentifiers

/// The place a photo was taken, which a camera writes into the picture
/// beside its pixels. A file field sends a photo without it unless the field
/// says to keep it (`keep_location`): a person who posts a picture of a
/// receipt has not said where they were.
///
/// The place is taken out as the picked file is copied for the form, so the
/// bytes the app keeps to send never hold it. A picture that holds none is
/// copied as it is, byte for byte. One that holds one is written again
/// without it: first with its pixels untouched, where the system can do
/// that for its format, else drawn into a new file of the same format. What
/// was written is then read back, and a copy that still names a place, or
/// does not read as a picture, is thrown away: a photo whose place cannot be
/// taken out is not kept for the form at all, never sent as it was.
///
/// What goes is what the system calls the picture's GPS data, in its EXIF and
/// in its XMP: where it was taken, how high, which way the camera looked,
/// and the satellites' clock. What a camera's maker writes in a part of its
/// own, and the names of places somebody typed into a picture's caption
/// fields, the system does not take out, and neither does this. A file that
/// is not a picture (a document, a film) is copied as it is.
enum PhotoPlace {
    /// Copies the file at `source` to `copy` for a form to send: a picture
    /// without the place it was taken, unless `keepingPlace`. `false` when it
    /// could not be copied, or is a picture whose place could not be taken
    /// out; nothing is left at `copy` then.
    static func copy(_ source: URL, to copy: URL, keepingPlace: Bool) -> Bool {
        let done = keepingPlace || !isPicture(source) ? plain(source, to: copy) : withoutPlace(source, to: copy)
        if !done { try? FileManager.default.removeItem(at: copy) }
        return done
    }

    /// Whether a file is a picture: by its name, or by what the system reads
    /// in it whatever its name.
    static func isPicture(_ url: URL) -> Bool {
        if let named = UTType(filenameExtension: url.pathExtension), named.conforms(to: .image) { return true }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let type = CGImageSourceGetType(source)
        else { return false }
        return UTType(type as String)?.conforms(to: .image) ?? false
    }

    /// Whether a picture names a place: GPS data among the properties of any
    /// image in the file, or a GPS tag in the XMP written beside one.
    static func holdsPlace(_ source: CGImageSource) -> Bool {
        for index in 0..<CGImageSourceGetCount(source) {
            if let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [String: Any],
                properties[kCGImagePropertyGPSDictionary as String] != nil
            {
                return true
            }
            guard let metadata = CGImageSourceCopyMetadataAtIndex(source, index, nil),
                let tags = CGImageMetadataCopyTags(metadata) as? [CGImageMetadataTag]
            else { continue }
            for tag in tags {
                let name = CGImageMetadataTagCopyName(tag) as String? ?? ""
                if name.hasPrefix("GPS") { return true }
            }
        }
        return false
    }

    private static func plain(_ source: URL, to copy: URL) -> Bool {
        do {
            try FileManager.default.copyItem(at: source, to: copy)
            return true
        } catch {
            return false
        }
    }

    /// Copies a picture without its place. A file that is a picture by its
    /// name and does not read as one cannot be looked into, so it is not copied.
    private static func withoutPlace(_ url: URL, to copy: URL) -> Bool {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let type = CGImageSourceGetType(source),
            CGImageSourceGetCount(source) > 0
        else { return false }
        if !holdsPlace(source) { return plain(url, to: copy) }
        if rewrite(source, as: type, to: copy, pixelsUntouched: true), isWithoutPlace(copy) { return true }
        try? FileManager.default.removeItem(at: copy)
        return rewrite(source, as: type, to: copy, pixelsUntouched: false) && isWithoutPlace(copy)
    }

    /// Writes the picture again in its own format, leaving the place out.
    private static func rewrite(_ source: CGImageSource, as type: CFString, to copy: URL, pixelsUntouched: Bool) -> Bool {
        let count = CGImageSourceGetCount(source)
        guard let destination = CGImageDestinationCreateWithURL(copy as CFURL, type, count, nil) else { return false }
        if pixelsUntouched {
            // The picture's own data as it is, and what is written beside it without the place.
            let options = [kCGImageMetadataShouldExcludeGPS: true] as CFDictionary
            return CGImageDestinationCopyImageSource(destination, source, options, nil)
        }
        // Drawn into a new file: the place is neither copied nor written.
        let properties: [CFString: Any] = [kCGImageMetadataShouldExcludeGPS: true, kCGImagePropertyGPSDictionary: NSNull()]
        for index in 0..<count {
            CGImageDestinationAddImageFromSource(destination, source, index, properties as CFDictionary)
        }
        return CGImageDestinationFinalize(destination)
    }

    /// Whether what was written reads as a picture and names no place.
    private static func isWithoutPlace(_ copy: URL) -> Bool {
        guard let written = CGImageSourceCreateWithURL(copy as CFURL, nil), CGImageSourceGetType(written) != nil,
            CGImageSourceGetCount(written) > 0
        else { return false }
        return !holdsPlace(written)
    }
}
