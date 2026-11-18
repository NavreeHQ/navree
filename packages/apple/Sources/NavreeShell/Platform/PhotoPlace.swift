import Foundation
import ImageIO
import UniformTypeIdentifiers

/// The place a photo was taken, which a camera, a phone or a drone writes
/// into the picture beside its pixels. A file field sends a photo without it
/// unless the field says to keep it (`keep_location`): a person who posts a
/// picture of a receipt has not said where they were.
///
/// A place can be written in more ways than one: as the picture's GPS data;
/// in its XMP packet, at the top or inside a structure, under the standard
/// names or a maker's own (a drone's `GpsLatitude`); in a maker's notes; in
/// the record of caption fields, as the name of a town. No list of names
/// covers them all. So a picture is not searched for the names known to say
/// a place: what could hold one is left out whole, as far as Image I/O, the
/// system's own way of reading a picture, shows it. What is kept of a
/// picture is its pixels, which way up it is, and a short list of what a
/// camera says of the shot (when, with what, at which exposure). The GPS
/// data, the whole XMP packet, the notes of every maker Image I/O shows a
/// group for, the caption record and every other field it shows go.
///
/// The check sees what Image I/O reads, and the marks of an XMP packet in
/// the file's bytes. A place hidden where Image I/O does not look is not
/// seen, and is not taken out: a PNG's compressed text, a maker's bytes after
/// the picture's end, content credentials, a maker's notes it does not show.
/// A picture the check finds nothing in is copied as it is, with whatever
/// else it holds. And since anything of what is left out makes a picture one
/// to write again, a picture in a format the system reads and cannot write
/// is refused whether or not a place is in it.
///
/// It is done as the picked file is copied for the form, so the bytes the app
/// keeps to send never hold it:
///
/// - A picture that holds none of those is copied as it is, byte for byte.
/// - Any other is written again without them: first with its pixels
///   untouched, where the system can do that for its format; else drawn
///   upright into a new file of the same format, with nothing beside its
///   pixels.
/// - What was written is read back. A copy that still holds any of them, or
///   does not read as a picture, is thrown away and the next way is tried.
/// - A picture that cannot be written clean either way is not kept for the
///   form at all (``Outcome/placeStays``): it is never sent as it was.
///
/// The file is read once, and what is looked into is what is written from. A
/// file that is not a picture (a document, a film) is copied as it is.
enum PhotoPlace {
    /// What became of a picked file as it was copied for a form.
    enum Outcome: Equatable, Sendable {
        /// It was copied: as it is, or a picture without what could say where it was taken.
        case copied
        /// It could not be read, or the copy could not be written.
        case unreadable
        /// A picture by its name that does not read as one: whether it says
        /// where it was taken cannot be told, so it is not copied.
        case unchecked
        /// A picture that holds what could say where it was taken, in a kind
        /// of file it cannot be taken out of: it is not copied.
        case placeStays
    }

    /// Copies the file at `source` to `copy` for a form to send: a picture
    /// without what could say where it was taken, unless `keepingPlace`.
    /// Whatever the outcome but ``Outcome/copied``, nothing is left at `copy`.
    static func copy(_ source: URL, to copy: URL, keepingPlace: Bool) -> Outcome {
        let outcome: Outcome
        if keepingPlace || !isPicture(source) {
            outcome = plain(source, to: copy) ? .copied : .unreadable
        } else {
            outcome = withoutPlace(source, to: copy)
        }
        if outcome != .copied { try? FileManager.default.removeItem(at: copy) }
        return outcome
    }

    /// Whether a file is a picture: by its name, or by what the system reads
    /// in it whatever its name.
    static func isPicture(_ url: URL) -> Bool {
        if let named = UTType(filenameExtension: url.pathExtension), named.conforms(to: .image) { return true }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let type = CGImageSourceGetType(source)
        else { return false }
        return UTType(type as String)?.conforms(to: .image) ?? false
    }

    /// Whether the bytes of a picture hold nothing that could say where it
    /// was taken: no GPS data, no maker's notes, no caption record, no free
    /// words of the camera's owner, no XMP packet at all, and no field
    /// anywhere, at any depth and under any maker's name, that is named for
    /// a place. `false` for bytes that do not read as a picture.
    static func isBare(_ bytes: Data) -> Bool {
        guard let source = CGImageSourceCreateWithData(bytes as CFData, nil), CGImageSourceGetType(source) != nil,
            CGImageSourceGetCount(source) > 0
        else { return false }
        return isBare(source, bytes: bytes)
    }

    private static func isBare(_ source: CGImageSource, bytes: Data) -> Bool {
        if holdsPacket(bytes) { return false }
        for index in 0..<CGImageSourceGetCount(source) {
            if let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [String: Any] {
                if properties.keys.contains(where: { holdsPlace($0) }) { return false }
                if let exif = properties[kCGImagePropertyExifDictionary as String] as? [String: Any],
                    exif[kCGImagePropertyExifMakerNote as String] != nil || exif[kCGImagePropertyExifUserComment as String] != nil
                {
                    return false
                }
            }
            if let metadata = CGImageSourceCopyMetadataAtIndex(source, index, nil), namesPlace(metadata) { return false }
        }
        return true
    }

    /// Whether a group of a picture's properties is one that can say where
    /// it was taken: its GPS data, a maker's notes (each maker has a group
    /// of its own), the caption record.
    static func holdsPlace(_ group: String) -> Bool {
        group == kCGImagePropertyGPSDictionary as String || group == kCGImagePropertyIPTCDictionary as String
            || group.hasPrefix("{Maker")
    }

    /// Whether a file's bytes hold an XMP packet, in any of the ways a
    /// picture carries one.
    static func holdsPacket(_ bytes: Data) -> Bool {
        let marks = ["<x:xmpmeta", "<?xpacket", "http://ns.adobe.com/xap/1.0/", "XML:com.adobe.xmp", "<rdf:RDF"]
        return marks.contains { bytes.range(of: Data($0.utf8)) != nil }
    }

    /// Whether any field of a picture's metadata, at any depth and under any
    /// name a maker gives it, is named for a place.
    private static func namesPlace(_ metadata: CGImageMetadata) -> Bool {
        var found = false
        let everywhere = [kCGImageMetadataEnumerateRecursively: true] as CFDictionary
        CGImageMetadataEnumerateTagsUsingBlock(metadata, nil, everywhere) { path, _ in
            let name = (path as String).lowercased()
            if name.contains("gps") || name.contains("latitude") || name.contains("longitude") || name.contains("location") {
                found = true
            }
            return !found
        }
        return found
    }

    private static func plain(_ source: URL, to copy: URL) -> Bool {
        do {
            try FileManager.default.copyItem(at: source, to: copy)
            return true
        } catch {
            return false
        }
    }

    /// Copies a picture without what could say where it was taken.
    private static func withoutPlace(_ url: URL, to copy: URL) -> Outcome {
        // Read once: what is looked into is what is written from.
        guard let bytes = try? Data(contentsOf: url) else { return .unreadable }
        guard let source = CGImageSourceCreateWithData(bytes as CFData, nil), let type = CGImageSourceGetType(source),
            CGImageSourceGetCount(source) > 0
        else { return .unchecked }
        if isBare(source, bytes: bytes) {
            return (try? bytes.write(to: copy)) != nil ? .copied : .unreadable
        }
        for pixelsUntouched in [true, false] {
            try? FileManager.default.removeItem(at: copy)
            guard rewrite(source, as: type, to: copy, pixelsUntouched: pixelsUntouched),
                let written = try? Data(contentsOf: copy), isBare(written)
            else { continue }
            return .copied
        }
        return .placeStays
    }

    /// Writes the picture again in its own format. With `pixelsUntouched`,
    /// its own data as it is, and beside it only the fields kept
    /// (``kept(of:)``): no GPS data, no XMP packet, nothing else of what was
    /// there. Without, every image of it drawn upright into a new file, with
    /// nothing beside the pixels.
    private static func rewrite(_ source: CGImageSource, as type: CFString, to copy: URL, pixelsUntouched: Bool) -> Bool {
        let count = CGImageSourceGetCount(source)
        guard let destination = CGImageDestinationCreateWithURL(copy as CFURL, type, count, nil) else { return false }
        if pixelsUntouched {
            let options: [CFString: Any] = [
                kCGImageDestinationMetadata: kept(of: source), kCGImageDestinationMergeMetadata: false,
                kCGImageMetadataShouldExcludeXMP: true, kCGImageMetadataShouldExcludeGPS: true,
            ]
            return CGImageDestinationCopyImageSource(destination, source, options as CFDictionary, nil)
        }
        let quality: [CFString: Any] = [kCGImageDestinationLossyCompressionQuality: 0.9]
        for index in 0..<count {
            guard let image = upright(source, at: index) else { return false }
            CGImageDestinationAddImage(destination, image, quality as CFDictionary)
        }
        return CGImageDestinationFinalize(destination)
    }

    /// What is kept of a picture's fields when it is written again with its
    /// pixels untouched: which way up it is, how finely it prints, when it
    /// was taken, with what camera and lens and at which exposure. A short
    /// list of what cannot say a place; whatever is not on it goes.
    private static func kept(of source: CGImageSource) -> CGImageMetadata {
        let metadata = CGImageMetadataCreateMutable()
        guard let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] else { return metadata }
        let kept: [(group: CFString, names: [CFString])] = [
            (
                kCGImagePropertyTIFFDictionary,
                [
                    kCGImagePropertyTIFFOrientation, kCGImagePropertyTIFFXResolution, kCGImagePropertyTIFFYResolution,
                    kCGImagePropertyTIFFResolutionUnit, kCGImagePropertyTIFFMake, kCGImagePropertyTIFFModel,
                    kCGImagePropertyTIFFDateTime,
                ]
            ),
            (
                kCGImagePropertyExifDictionary,
                [
                    kCGImagePropertyExifDateTimeOriginal, kCGImagePropertyExifDateTimeDigitized,
                    kCGImagePropertyExifExposureTime, kCGImagePropertyExifFNumber, kCGImagePropertyExifISOSpeedRatings,
                    kCGImagePropertyExifFocalLength, kCGImagePropertyExifFlash, kCGImagePropertyExifLensModel,
                    kCGImagePropertyExifColorSpace, kCGImagePropertyExifPixelXDimension,
                    kCGImagePropertyExifPixelYDimension,
                ]
            ),
        ]
        for (group, names) in kept {
            guard let values = properties[group as String] as? [String: Any] else { continue }
            for name in names {
                guard let value = values[name as String] else { continue }
                _ = CGImageMetadataSetValueMatchingImageProperty(metadata, group, name, value as AnyObject)
            }
        }
        return metadata
    }

    /// One image of a picture, drawn the way up it is shown, at its own size.
    private static func upright(_ source: CGImageSource, at index: Int) -> CGImage? {
        guard let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [String: Any],
            let width = properties[kCGImagePropertyPixelWidth as String] as? Int,
            let height = properties[kCGImagePropertyPixelHeight as String] as? Int
        else { return CGImageSourceCreateImageAtIndex(source, index, nil) }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max(width, height),
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, index, options as CFDictionary)
    }
}
