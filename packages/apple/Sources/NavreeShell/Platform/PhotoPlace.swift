import CoreGraphics
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
/// the record of caption fields, as the name of a town; in a PNG's compressed
/// profile chunk; or in bytes appended after the picture ends, such as a
/// maker's trailer or a motion photo's film.
///
/// So a picture is checked both through Image I/O and through the bytes of its
/// container:
///
/// - Every image in the file must yield readable properties, with no GPS
///   record, no caption record, no maker group, no maker note, no owner
///   comment or free description, no time-zone offset, no XMP packet anywhere
///   in the file's bytes, and no metadata tag at any depth named for a place.
/// - A picture is kept byte for byte only when its container is JPEG, PNG or
///   HEIC holding only known structure, with clean Exif fields and no bytes
///   after the picture ends.
/// - Any other picture is written again without them: first with its pixels
///   untouched, where the system can do that for its format; when what the
///   first way wrote fails the same check, it is thrown away and every image
///   is drawn upright into a new file of the same format with nothing beside
///   its pixels.
/// - A picture that cannot be written clean either way — such as a layered
///   design file or a camera raw — is not kept for the form at all
///   (``Outcome/placeStays``): it is never sent as it was.
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
        /// A picture in a format from which details that could say where it
        /// was taken cannot be removed: it is not copied.
        case placeStays
    }

    /// Whether Image I/O properties are read normally or treated as unreadable in tests.
    enum PropertyReading: Sendable {
        case system
        case unreadableOriginal
        case alwaysUnreadable
    }

    typealias Rewriter = @Sendable (_ sourceBytes: Data, _ to: URL, _ pixelsUntouched: Bool) -> Bool

    static let defaultRewriter: Rewriter = { sourceBytes, copy, pixelsUntouched in
        guard let source = CGImageSourceCreateWithData(sourceBytes as CFData, nil),
            let type = CGImageSourceGetType(source),
            CGImageSourceGetCount(source) > 0
        else { return false }
        return rewrite(source, as: type, to: copy, pixelsUntouched: pixelsUntouched)
    }

    /// Formats Image I/O can write as new files on this device.
    private static let writableTypes: Set<String> = {
        Set(CGImageDestinationCopyTypeIdentifiers() as? [String] ?? [])
    }()

    /// Container formats whose structure ``hasKnownContainer(_:)`` verifies
    /// byte for byte. Other image formats (such as GIF, TIFF, BMP, WebP or
    /// AVIF) are refused before rewriting unless `keep_location` is set.
    private static let verifiedContainerTypes: Set<String> = [
        UTType.jpeg.identifier, UTType.png.identifier, UTType.heic.identifier,
    ]

    /// Extensions of layered design files and camera raws, which Image I/O
    /// either cannot write or would flatten if rewritten.
    private static let designAndRawExtensions: Set<String> = [
        "psd", "psb", "svg", "svgz", "ai", "eps",
        "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2",
        "raf", "orf", "rw2", "pef", "srw", "iiq", "3fr", "erf", "kdc", "mef", "mos", "mrw", "x3f",
    ]

    /// Copies the file at `source` to `copy` for a form to send: a picture
    /// without what could say where it was taken, unless `keepingPlace`.
    /// Whatever the outcome but ``Outcome/copied``, nothing is left at `copy`.
    static func copy(
        _ source: URL, to copy: URL, keepingPlace: Bool,
        propertyReading: PropertyReading = .system,
        rewriter: Rewriter = defaultRewriter
    ) -> Outcome {
        try? FileManager.default.removeItem(at: copy)
        if keepingPlace {
            return plain(source, to: copy) ? .copied : .unreadable
        }
        // Stage the source once in a private temporary file so a file provider
        // cannot swap the file between checking what it is and copying it.
        let ext = source.pathExtension
        let stagedBase = FileManager.default.temporaryDirectory
            .appendingPathComponent("navree-stage-\(UUID().uuidString)")
        let staged = ext.isEmpty ? stagedBase : stagedBase.appendingPathExtension(ext)
        guard plain(source, to: staged) else { return .unreadable }
        defer { try? FileManager.default.removeItem(at: staged) }

        let outcome: Outcome
        if !isPicture(staged, originalExtension: ext) {
            outcome = (try? FileManager.default.moveItem(at: staged, to: copy)) != nil ? .copied : .unreadable
        } else {
            outcome = withoutPlace(
                staged, originalExtension: ext, to: copy, propertyReading: propertyReading, rewriter: rewriter
            )
        }
        if outcome != .copied { try? FileManager.default.removeItem(at: copy) }
        return outcome
    }

    /// Whether a file is a picture: by its name, by a design-file signature,
    /// or by what the system reads in it whatever its name.
    static func isPicture(_ url: URL, originalExtension: String? = nil) -> Bool {
        let ext = (originalExtension ?? url.pathExtension).lowercased()
        if designAndRawExtensions.contains(ext) { return true }
        if !ext.isEmpty, let named = UTType(filenameExtension: ext), named.conforms(to: .image) { return true }
        if let head = prefix(of: url, count: 64), isDesignSignature(head) { return true }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let type = CGImageSourceGetType(source)
        else { return false }
        return UTType(type as String)?.conforms(to: .image) ?? false
    }

    /// Whether a file is a layered design file or a camera raw by its name or bytes.
    static func isDesignOrRaw(extension ext: String, typeIdentifier: String? = nil, bytes: Data? = nil) -> Bool {
        let lower = ext.lowercased()
        if designAndRawExtensions.contains(lower) { return true }
        if !lower.isEmpty, let named = UTType(filenameExtension: lower) {
            if named.conforms(to: .rawImage) || named.conforms(to: .svg)
                || named.identifier == "com.adobe.photoshop-image"
            {
                return true
            }
        }
        if let typeIdentifier {
            if typeIdentifier == "com.adobe.photoshop-image" { return true }
            if let kind = UTType(typeIdentifier), kind.conforms(to: .rawImage) || kind.conforms(to: .svg) {
                return true
            }
        }
        if let bytes, isDesignSignature(bytes) { return true }
        return false
    }

    /// Whether the bytes of a picture hold nothing that could say where it
    /// was taken, and sit in a known, clean JPEG, PNG or HEIC container with
    /// readable properties for every image. `false` for bytes that do not read
    /// as a picture or whose properties cannot be read.
    static func isBare(_ bytes: Data, readableProperties: Bool = true) -> Bool {
        guard let source = CGImageSourceCreateWithData(bytes as CFData, nil), CGImageSourceGetType(source) != nil,
            CGImageSourceGetCount(source) > 0
        else { return false }
        return isBare(source, bytes: bytes, readableProperties: readableProperties)
    }

    private static func isBare(
        _ source: CGImageSource, bytes: Data, readableProperties: Bool = true
    ) -> Bool {
        if !readableProperties { return false }
        if holdsPacket(bytes) { return false }
        if !hasKnownContainer(bytes) { return false }
        let count = CGImageSourceGetCount(source)
        guard count > 0 else { return false }
        for index in 0..<count {
            let props = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [String: Any]
            guard arePropertiesBare(props) else { return false }
            if let metadata = CGImageSourceCopyMetadataAtIndex(source, index, nil), namesPlace(metadata) {
                return false
            }
        }
        return true
    }

    /// Whether a single image's property dictionary read cleanly and holds no
    /// group or field that can say where the picture was taken.
    static func arePropertiesBare(_ properties: [String: Any]?) -> Bool {
        guard let properties, !properties.isEmpty else { return false }
        if properties.keys.contains(where: { holdsPlace($0) }) { return false }
        if let exif = properties[kCGImagePropertyExifDictionary as String] as? [String: Any] {
            for forbidden in forbiddenExifKeys where exif[forbidden] != nil { return false }
        }
        if let tiff = properties[kCGImagePropertyTIFFDictionary as String] as? [String: Any] {
            for forbidden in forbiddenTIFFKeys where tiff[forbidden] != nil { return false }
        }
        if let png = properties["{PNG}"] as? [String: Any] {
            for forbidden in forbiddenPNGKeys where png[forbidden] != nil { return false }
        }
        return true
    }

    private static let forbiddenExifKeys: [String] = [
        kCGImagePropertyExifMakerNote as String,
        kCGImagePropertyExifUserComment as String,
        "OffsetTime", "OffsetTimeOriginal", "OffsetTimeDigitized",
        "ImageUniqueID", "CameraOwnerName", "BodySerialNumber", "LensSerialNumber",
        "SubjectLocation", "SubjectArea",
    ]

    private static let forbiddenTIFFKeys: [String] = [
        "ImageDescription", "Artist", "Copyright", "HostComputer", "DocumentName", "PageName",
    ]

    private static let forbiddenPNGKeys: [String] = [
        "Description", "Comment", "Author", "Copyright", "Title", "Disclaimer", "Warning", "Source", "Creation Time",
    ]

    /// Whether a group of a picture's properties is one that can say where
    /// it was taken: its GPS data, a maker's notes, the caption record,
    /// auxiliary Exif, raw or Photoshop resources.
    static func holdsPlace(_ group: String) -> Bool {
        group == kCGImagePropertyGPSDictionary as String
            || group == kCGImagePropertyIPTCDictionary as String
            || group.hasPrefix("{Maker")
            || group == "{ExifAux}"
            || group == "{8BIM}"
            || group == "{CIFF}"
            || group == "{DNG}"
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
            if name.contains("gps") || name.contains("latitude") || name.contains("longitude")
                || name.contains("location")
            {
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

    private static func prefix(of url: URL, count: Int) -> Data? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        return try? handle.read(upToCount: count)
    }

    private static func isDesignSignature(_ data: Data) -> Bool {
        if data.starts(with: [0x38, 0x42, 0x50, 0x53]) { return true }
        let trimmed = String(decoding: data.prefix(64), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        return trimmed.hasPrefix("<svg") || (trimmed.hasPrefix("<?xml") && trimmed.contains("<svg"))
    }

    /// Copies a staged picture without what could say where it was taken.
    static func withoutPlace(
        _ url: URL, originalExtension: String? = nil, to copy: URL,
        propertyReading: PropertyReading = .system,
        rewriter: Rewriter = defaultRewriter
    ) -> Outcome {
        let ext = (originalExtension ?? url.pathExtension).lowercased()
        guard let bytes = try? Data(contentsOf: url) else { return .unreadable }
        if isDesignOrRaw(extension: ext, bytes: bytes) { return .placeStays }

        guard let source = CGImageSourceCreateWithData(bytes as CFData, nil), let type = CGImageSourceGetType(source),
            CGImageSourceGetCount(source) > 0
        else {
            if !ext.isEmpty, let named = UTType(filenameExtension: ext), named.conforms(to: .image),
                !verifiedContainerTypes.contains(named.identifier) || !writableTypes.contains(named.identifier)
            {
                return .placeStays
            }
            return .unchecked
        }

        let typeID = type as String
        if isDesignOrRaw(extension: ext, typeIdentifier: typeID, bytes: bytes)
            || !verifiedContainerTypes.contains(typeID)
            || !writableTypes.contains(typeID)
        {
            return .placeStays
        }

        let originalReadable = propertyReading == .system
        if isBare(source, bytes: bytes, readableProperties: originalReadable) {
            return (try? bytes.write(to: copy)) != nil ? .copied : .unreadable
        }

        let rewrittenReadable = propertyReading != .alwaysUnreadable
        for pixelsUntouched in [true, false] {
            try? FileManager.default.removeItem(at: copy)
            guard rewriter(bytes, copy, pixelsUntouched),
                let written = try? Data(contentsOf: copy),
                isBare(written, readableProperties: rewrittenReadable)
            else { continue }
            return .copied
        }
        try? FileManager.default.removeItem(at: copy)
        return .placeStays
    }

    /// Writes the picture again in its own format. With `pixelsUntouched`,
    /// its own data as it is, and beside it only the fields kept
    /// (``kept(of:)``): no GPS data, no XMP packet, nothing else of what was
    /// there. Without, every image of it drawn upright into a new file, with
    /// nothing beside the pixels.
    static func rewrite(_ source: CGImageSource, as type: CFString, to copy: URL, pixelsUntouched: Bool) -> Bool {
        let count = CGImageSourceGetCount(source)
        if pixelsUntouched {
            guard let destination = CGImageDestinationCreateWithURL(copy as CFURL, type, count, nil) else {
                return false
            }
            let options: [CFString: Any] = [
                kCGImageDestinationMetadata: kept(of: source), kCGImageDestinationMergeMetadata: false,
                kCGImageMetadataShouldExcludeXMP: true, kCGImageMetadataShouldExcludeGPS: true,
            ]
            return CGImageDestinationCopyImageSource(destination, source, options as CFDictionary, nil)
        }
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data as CFMutableData, type, count, nil) else {
            return false
        }
        let opaque = (type as String) == UTType.jpeg.identifier
        let empty: [CFString: Any] = [:]
        for index in 0..<count {
            guard let image = upright(source, at: index, forceOpaque: opaque) else { return false }
            CGImageDestinationAddImage(destination, image, empty as CFDictionary)
        }
        guard CGImageDestinationFinalize(destination) else { return false }
        return (try? (data as Data).write(to: copy)) != nil
    }

    /// What is kept of a picture's fields when it is written again with its
    /// pixels untouched: which way up it is, how finely it prints, when it
    /// was taken, with what camera and lens and at which exposure. A short
    /// list of what cannot say a place; whatever is not on it goes.
    private static func kept(of source: CGImageSource) -> CGImageMetadata {
        let metadata = CGImageMetadataCreateMutable()
        guard let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] else {
            return metadata
        }
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

    /// One image of a picture, drawn the way up it is shown, at its own size,
    /// into a fresh bitmap with no source metadata attached.
    private static func upright(_ source: CGImageSource, at index: Int, forceOpaque: Bool) -> CGImage? {
        let decoded: CGImage?
        if let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [String: Any],
            let width = properties[kCGImagePropertyPixelWidth as String] as? Int,
            let height = properties[kCGImagePropertyPixelHeight as String] as? Int
        {
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: max(width, height),
            ]
            decoded = CGImageSourceCreateThumbnailAtIndex(source, index, options as CFDictionary)
        } else {
            decoded = CGImageSourceCreateImageAtIndex(source, index, nil)
        }
        guard let decoded, decoded.width > 0, decoded.height > 0 else { return nil }
        let hasAlpha: Bool = switch decoded.alphaInfo {
        case .first, .last, .premultipliedFirst, .premultipliedLast, .alphaOnly: true
        default: false
        }
        let bitmapInfo = (!forceOpaque && hasAlpha)
            ? CGImageAlphaInfo.premultipliedLast.rawValue
            : CGImageAlphaInfo.noneSkipLast.rawValue
        guard let context = CGContext(
            data: nil, width: decoded.width, height: decoded.height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: bitmapInfo
        ) else { return nil }
        context.draw(decoded, in: CGRect(x: 0, y: 0, width: decoded.width, height: decoded.height))
        return context.makeImage()
    }

    // MARK: - Container structure

    /// Whether `data` is a JPEG, PNG or HEIC container holding only known
    /// structure, with clean Exif fields and zero bytes after its end.
    static func hasKnownContainer(_ data: Data) -> Bool {
        let bytes = [UInt8](data)
        if bytes.starts(with: [0xFF, 0xD8]) { return isCleanJPEG(bytes) }
        if bytes.starts(with: pngSignature) { return isCleanPNG(bytes) }
        if bytes.count >= 12, fourCC(bytes, at: 4) == "ftyp" { return isCleanHEIC(bytes) }
        return false
    }

    private static let pngSignature: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]

    private static func isCleanJPEG(_ bytes: [UInt8]) -> Bool {
        guard bytes.count >= 4 else { return false }
        var cursor = 2
        var exifCount = 0
        var sawSOF = false
        var sawSOS = false

        while cursor < bytes.count {
            guard bytes[cursor] == 0xFF else { return false }
            while cursor < bytes.count && bytes[cursor] == 0xFF { cursor += 1 }
            guard cursor < bytes.count else { return false }
            let marker = bytes[cursor]
            cursor += 1
            if marker == 0x00 || marker == 0xD8 || marker == 0x01 || (0xD0...0xD7).contains(marker) {
                return false
            }
            if marker == 0xD9 {
                return sawSOF && sawSOS && cursor == bytes.count
            }
            guard cursor + 2 <= bytes.count else { return false }
            let segLen = (Int(bytes[cursor]) << 8) | Int(bytes[cursor + 1])
            guard segLen >= 2, cursor + segLen <= bytes.count else { return false }
            let payloadStart = cursor + 2
            let payloadEnd = cursor + segLen

            switch marker {
            case 0xE0:
                // APP0: JFIF only.
                guard hasPrefix(bytes, at: payloadStart, end: payloadEnd, prefix: [0x4A, 0x46, 0x49, 0x46, 0x00]) else {
                    return false
                }
                cursor = payloadEnd
            case 0xE1:
                // APP1: at most one Exif block, with only allowlisted TIFF/Exif fields.
                exifCount += 1
                guard exifCount == 1,
                    hasPrefix(bytes, at: payloadStart, end: payloadEnd, prefix: [0x45, 0x78, 0x69, 0x66, 0x00, 0x00]),
                    isCleanTIFF(bytes, start: payloadStart + 6, end: payloadEnd)
                else { return false }
                cursor = payloadEnd
            case 0xE2:
                // APP2: ICC colour profile only.
                let icc: [UInt8] = [0x49, 0x43, 0x43, 0x5F, 0x50, 0x52, 0x4F, 0x46, 0x49, 0x4C, 0x45, 0x00]
                guard hasPrefix(bytes, at: payloadStart, end: payloadEnd, prefix: icc) else { return false }
                cursor = payloadEnd
            case 0xEE:
                // APP14: Adobe colour transform marker only.
                guard hasPrefix(bytes, at: payloadStart, end: payloadEnd, prefix: [0x41, 0x64, 0x6F, 0x62, 0x65]) else {
                    return false
                }
                cursor = payloadEnd
            case 0xDB, 0xC4, 0xCC, 0xDD:
                cursor = payloadEnd
            case 0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF:
                sawSOF = true
                cursor = payloadEnd
            case 0xDA:
                sawSOS = true
                cursor = payloadEnd
                while cursor < bytes.count {
                    if bytes[cursor] != 0xFF {
                        cursor += 1
                        continue
                    }
                    while cursor < bytes.count && bytes[cursor] == 0xFF { cursor += 1 }
                    guard cursor < bytes.count else { return false }
                    let next = bytes[cursor]
                    if next == 0x00 || (0xD0...0xD7).contains(next) {
                        cursor += 1
                        continue
                    }
                    cursor -= 1
                    break
                }
            default:
                return false
            }
        }
        return false
    }

    private static let cleanPNGChunks: Set<String> = [
        "IHDR", "PLTE", "IDAT", "IEND", "tRNS", "bKGD", "sBIT",
        "iCCP", "sRGB", "gAMA", "cHRM", "pHYs", "cICP", "mDCV", "cLLI",
        "acTL", "fcTL", "fdAT",
    ]

    private static func isCleanPNG(_ bytes: [UInt8]) -> Bool {
        guard bytes.count >= 33 else { return false }
        var cursor = 8
        var sawIHDR = false
        var sawIDAT = false
        var exifCount = 0

        while cursor < bytes.count {
            guard cursor + 12 <= bytes.count else { return false }
            let length = u32BE(bytes, at: cursor)
            guard length <= 0x7FFF_FFFF else { return false }
            let chunkLen = Int(length)
            guard chunkLen <= bytes.count - cursor - 12 else { return false }
            let kind = fourCC(bytes, at: cursor + 4)
            let payloadStart = cursor + 8
            let payloadEnd = payloadStart + chunkLen
            let nextCursor = payloadEnd + 4

            if !sawIHDR {
                guard kind == "IHDR", chunkLen == 13 else { return false }
                sawIHDR = true
            } else if kind == "IHDR" {
                return false
            } else if kind == "IEND" {
                return chunkLen == 0 && sawIDAT && nextCursor == bytes.count
            } else if kind == "eXIf" {
                exifCount += 1
                guard exifCount == 1, isCleanTIFF(bytes, start: payloadStart, end: payloadEnd) else { return false }
            } else if cleanPNGChunks.contains(kind) {
                if kind == "IDAT" { sawIDAT = true }
            } else {
                return false
            }
            cursor = nextCursor
        }
        return false
    }

    private static let heicBrands: Set<String> = ["heic", "heix", "heim", "heis", "mif1", "msf1"]
    private static let cleanHEICTopBoxes: Set<String> = ["ftyp", "meta", "mdat", "free", "skip", "wide"]
    private static let cleanHEICMetaBoxes: Set<String> = [
        "hdlr", "dinf", "pitm", "iinf", "iref", "iprp", "idat", "iloc", "grpl", "free", "skip",
    ]
    private static let cleanHEICItemTypes: Set<String> = [
        "hvc1", "av01", "jpeg", "grid", "iden", "iovl", "tmap", "auxv",
    ]

    private static func isCleanHEIC(_ bytes: [UInt8]) -> Bool {
        var cursor = 0
        var sawFtyp = false
        var sawMeta = false
        var sawMdat = false

        while cursor < bytes.count {
            guard let box = readBox(bytes, at: cursor, limit: bytes.count) else { return false }
            guard cleanHEICTopBoxes.contains(box.kind) else { return false }
            switch box.kind {
            case "ftyp":
                guard box.payloadEnd - box.payloadStart >= 8 else { return false }
                let major = fourCC(bytes, at: box.payloadStart)
                guard heicBrands.contains(major) else { return false }
                sawFtyp = true
            case "meta":
                guard !sawMeta, isCleanHEICMeta(bytes, start: box.payloadStart, end: box.payloadEnd) else {
                    return false
                }
                sawMeta = true
            case "mdat":
                sawMdat = true
            case "free", "skip":
                for index in box.payloadStart..<box.payloadEnd where bytes[index] != 0 { return false }
            default:
                break
            }
            cursor = box.end
        }
        return sawFtyp && sawMeta && sawMdat && cursor == bytes.count
    }

    private static func isCleanHEICMeta(_ bytes: [UInt8], start: Int, end: Int) -> Bool {
        guard start + 4 <= end, bytes[start] == 0 else { return false }
        var cursor = start + 4
        var exifItemIDs: Set<UInt32> = []
        var ilocRange: (start: Int, end: Int)?
        var idatRange: (start: Int, end: Int)?

        while cursor < end {
            guard let box = readBox(bytes, at: cursor, limit: end), cleanHEICMetaBoxes.contains(box.kind) else {
                return false
            }
            switch box.kind {
            case "iinf":
                guard let ids = parseHEICItemInfo(bytes, start: box.payloadStart, end: box.payloadEnd) else {
                    return false
                }
                exifItemIDs.formUnion(ids)
            case "iloc":
                ilocRange = (box.payloadStart, box.payloadEnd)
            case "idat":
                idatRange = (box.payloadStart, box.payloadEnd)
            case "free", "skip":
                for index in box.payloadStart..<box.payloadEnd where bytes[index] != 0 { return false }
            default:
                break
            }
            cursor = box.end
        }
        guard cursor == end else { return false }
        if exifItemIDs.isEmpty { return true }
        guard let ilocRange,
            let locations = parseHEICLocations(
                bytes, start: ilocRange.start, end: ilocRange.end, idat: idatRange
            )
        else { return false }
        for itemID in exifItemIDs {
            guard let itemBytes = locations[itemID], isCleanHEICExif(itemBytes) else { return false }
        }
        return true
    }

    /// Parses `iinf` and returns the item IDs of any `Exif` items, or `nil`
    /// when `mime` (XMP) or an unknown item kind is present.
    private static func parseHEICItemInfo(_ bytes: [UInt8], start: Int, end: Int) -> Set<UInt32>? {
        guard start + 6 <= end else { return nil }
        let version = bytes[start]
        var cursor: Int
        let entryCount: Int
        if version == 0 {
            entryCount = Int(u16BE(bytes, at: start + 4))
            cursor = start + 6
        } else {
            guard start + 8 <= end else { return nil }
            let raw = u32BE(bytes, at: start + 4)
            guard raw <= 4096 else { return nil }
            entryCount = Int(raw)
            cursor = start + 8
        }
        var exifIDs: Set<UInt32> = []
        var seen = 0
        while cursor < end && seen < entryCount {
            guard let box = readBox(bytes, at: cursor, limit: end), box.kind == "infe" else { return nil }
            let p = box.payloadStart
            guard p + 4 <= box.payloadEnd else { return nil }
            let infeVersion = bytes[p]
            let itemID: UInt32
            let itemType: String
            if infeVersion == 2 {
                guard p + 12 <= box.payloadEnd else { return nil }
                itemID = UInt32(u16BE(bytes, at: p + 4))
                itemType = fourCC(bytes, at: p + 8)
            } else if infeVersion == 3 {
                guard p + 14 <= box.payloadEnd else { return nil }
                itemID = u32BE(bytes, at: p + 4)
                itemType = fourCC(bytes, at: p + 10)
            } else {
                return nil
            }
            if itemType == "Exif" {
                exifIDs.insert(itemID)
            } else if !cleanHEICItemTypes.contains(itemType) {
                return nil
            }
            seen += 1
            cursor = box.end
        }
        guard seen == entryCount, cursor == end else { return nil }
        return exifIDs
    }

    private static func parseHEICLocations(
        _ bytes: [UInt8], start: Int, end: Int, idat: (start: Int, end: Int)?
    ) -> [UInt32: [UInt8]]? {
        guard start + 8 <= end else { return nil }
        let version = bytes[start]
        guard version <= 2 else { return nil }
        let offsetSize = Int(bytes[start + 4] >> 4)
        let lengthSize = Int(bytes[start + 4] & 0x0F)
        let baseOffsetSize = Int(bytes[start + 5] >> 4)
        let indexSize = (version == 1 || version == 2) ? Int(bytes[start + 5] & 0x0F) : 0
        for size in [offsetSize, lengthSize, baseOffsetSize, indexSize] where ![0, 4, 8].contains(size) {
            return nil
        }

        var cursor: Int
        let itemCount: Int
        if version < 2 {
            itemCount = Int(u16BE(bytes, at: start + 6))
            cursor = start + 8
        } else {
            guard start + 10 <= end else { return nil }
            let raw = u32BE(bytes, at: start + 6)
            guard raw <= 4096 else { return nil }
            itemCount = Int(raw)
            cursor = start + 10
        }

        var out: [UInt32: [UInt8]] = [:]
        for _ in 0..<itemCount {
            let itemID: UInt32
            if version < 2 {
                guard cursor + 2 <= end else { return nil }
                itemID = UInt32(u16BE(bytes, at: cursor))
                cursor += 2
            } else {
                guard cursor + 4 <= end else { return nil }
                itemID = u32BE(bytes, at: cursor)
                cursor += 4
            }
            var construction = 0
            if version == 1 || version == 2 {
                guard cursor + 2 <= end else { return nil }
                construction = Int(u16BE(bytes, at: cursor) & 0x000F)
                cursor += 2
            }
            guard cursor + 2 <= end else { return nil }
            let dataRef = u16BE(bytes, at: cursor)
            cursor += 2
            guard let baseOffset = readSizedInt(bytes, at: &cursor, end: end, size: baseOffsetSize) else {
                return nil
            }
            guard cursor + 2 <= end else { return nil }
            let extentCount = Int(u16BE(bytes, at: cursor))
            cursor += 2
            guard extentCount <= 256 else { return nil }

            var assembled: [UInt8] = []
            var resolvable = true
            for _ in 0..<extentCount {
                if (version == 1 || version == 2) && indexSize > 0 {
                    guard readSizedInt(bytes, at: &cursor, end: end, size: indexSize) != nil else { return nil }
                }
                guard let extOffset = readSizedInt(bytes, at: &cursor, end: end, size: offsetSize),
                    let extLength = readSizedInt(bytes, at: &cursor, end: end, size: lengthSize)
                else { return nil }
                let (itemStart, over1) = baseOffset.addingReportingOverflow(extOffset)
                let (itemEnd, over2) = itemStart.addingReportingOverflow(extLength)
                guard !over1, !over2, itemStart >= 0, extLength >= 0 else { return nil }

                if construction == 0 && dataRef == 0 {
                    if itemEnd <= bytes.count {
                        assembled.append(contentsOf: bytes[itemStart..<itemEnd])
                    } else {
                        resolvable = false
                    }
                } else if construction == 1, let idat {
                    let s = idat.start + itemStart
                    let e = idat.start + itemEnd
                    if s >= idat.start, e <= idat.end {
                        assembled.append(contentsOf: bytes[s..<e])
                    } else {
                        resolvable = false
                    }
                } else {
                    resolvable = false
                }
            }
            if resolvable { out[itemID] = assembled }
        }
        return out
    }

    private static func isCleanHEICExif(_ item: [UInt8]) -> Bool {
        guard item.count >= 12 else { return false }
        let headerOffset = u32BE(item, at: 0)
        let validPrefix =
            headerOffset == 0
            || (headerOffset == 6
                && hasPrefix(item, at: 4, end: item.count, prefix: [0x45, 0x78, 0x69, 0x66, 0x00, 0x00]))
        guard validPrefix else { return false }
        let tiffStart = 4 + Int(headerOffset)
        guard tiffStart + 8 <= item.count else { return false }
        return isCleanTIFF(item, start: tiffStart, end: item.count)
    }

    // MARK: - Strict TIFF / Exif check

    private static let allowedRootTIFFTags: Set<UInt16> = [
        0x00FE, 0x0100, 0x0101, 0x0102, 0x0103, 0x0106,
        0x010F, 0x0110, 0x0112, 0x0115, 0x011A, 0x011B, 0x011C, 0x0128,
        0x0132, 0x013D, 0x013E, 0x013F,
        0x0211, 0x0212, 0x0213, 0x0214,
        0x8769, 0x8773,
    ]

    private static let allowedExifIFDTags: Set<UInt16> = [
        0x829A, 0x829D, 0x8822, 0x8827, 0x8830, 0x8832,
        0x9000, 0x9003, 0x9004, 0x9101, 0x9102,
        0x9201, 0x9202, 0x9203, 0x9204, 0x9205, 0x9207, 0x9208, 0x9209, 0x920A,
        0x9290, 0x9291, 0x9292,
        0xA000, 0xA001, 0xA002, 0xA003, 0xA005,
        0xA20E, 0xA20F, 0xA210, 0xA217,
        0xA300, 0xA301, 0xA302,
        0xA401, 0xA402, 0xA403, 0xA404, 0xA405, 0xA406, 0xA407, 0xA408, 0xA409, 0xA40A,
        0xA432, 0xA434, 0xA460, 0xA500,
    ]

    private static let allowedInteropTags: Set<UInt16> = [0x0001, 0x0002]

    private static func isCleanTIFF(_ bytes: [UInt8], start: Int, end: Int) -> Bool {
        let length = end - start
        guard start >= 0, end <= bytes.count, length >= 8 else { return false }
        let little: Bool
        if bytes[start] == 0x49 && bytes[start + 1] == 0x49 {
            little = true
        } else if bytes[start] == 0x4D && bytes[start + 1] == 0x4D {
            little = false
        } else {
            return false
        }

        func u16(_ offset: Int) -> UInt16 {
            let p = start + offset
            return little
                ? (UInt16(bytes[p]) | (UInt16(bytes[p + 1]) << 8))
                : ((UInt16(bytes[p]) << 8) | UInt16(bytes[p + 1]))
        }

        func u32(_ offset: Int) -> UInt32 {
            let p = start + offset
            return little
                ? (UInt32(bytes[p]) | (UInt32(bytes[p + 1]) << 8) | (UInt32(bytes[p + 2]) << 16)
                    | (UInt32(bytes[p + 3]) << 24))
                : ((UInt32(bytes[p]) << 24) | (UInt32(bytes[p + 1]) << 16) | (UInt32(bytes[p + 2]) << 8)
                    | UInt32(bytes[p + 3]))
        }

        guard u16(2) == 0x002A else { return false }
        let rootOffset = u32(4)
        guard rootOffset >= 8, Int(rootOffset) < length else { return false }

        var covered = [Bool](repeating: false, count: length)
        for index in 0..<8 { covered[index] = true }
        var visited: Set<Int> = []

        enum Kind {
            case root
            case exif
            case interop
        }

        func checkIFD(at offset: Int, kind: Kind) -> Bool {
            guard !visited.contains(offset), visited.count < 8, offset >= 8, offset + 2 <= length else {
                return false
            }
            visited.insert(offset)
            let count = Int(u16(offset))
            guard count <= 256 else { return false }
            let tableEnd = offset + 2 + count * 12 + 4
            guard tableEnd <= length else { return false }
            for index in offset..<tableEnd { covered[index] = true }

            let allowed: Set<UInt16> = switch kind {
            case .root: allowedRootTIFFTags
            case .exif: allowedExifIFDTags
            case .interop: allowedInteropTags
            }

            for entry in 0..<count {
                let pos = offset + 2 + entry * 12
                let tag = u16(pos)
                let fieldType = u16(pos + 2)
                let rawCount = u32(pos + 4)
                guard allowed.contains(tag), let unit = tiffTypeSize(fieldType), rawCount <= UInt32(length) else {
                    return false
                }
                let (valueBytes, overflow) = Int(rawCount).multipliedReportingOverflow(by: unit)
                guard !overflow, valueBytes <= length else { return false }

                if valueBytes > 4 {
                    let valOffset = Int(u32(pos + 8))
                    guard valOffset >= 8, valOffset + valueBytes <= length else { return false }
                    for index in valOffset..<(valOffset + valueBytes) { covered[index] = true }
                }

                if tag == 0x8769 {
                    guard kind == .root, rawCount == 1 else { return false }
                    let sub = Int(u32(pos + 8))
                    guard checkIFD(at: sub, kind: .exif) else { return false }
                } else if tag == 0xA005 {
                    guard kind == .exif, rawCount == 1 else { return false }
                    let sub = Int(u32(pos + 8))
                    guard checkIFD(at: sub, kind: .interop) else { return false }
                }
            }

            // Do not allow a second IFD (such as an embedded IFD1 thumbnail).
            let nextIFD = u32(offset + 2 + count * 12)
            return nextIFD == 0
        }

        guard checkIFD(at: Int(rootOffset), kind: .root) else { return false }
        for index in 8..<length where !covered[index] && bytes[start + index] != 0 {
            return false
        }
        return true
    }

    private static func tiffTypeSize(_ fieldType: UInt16) -> Int? {
        switch fieldType {
        case 1, 2, 6, 7: 1
        case 3, 8: 2
        case 4, 9, 11, 13: 4
        case 5, 10, 12: 8
        default: nil
        }
    }

    // MARK: - Byte helpers

    private struct Box {
        let kind: String
        let payloadStart: Int
        let payloadEnd: Int
        var end: Int { payloadEnd }
    }

    private static func readBox(_ bytes: [UInt8], at offset: Int, limit: Int) -> Box? {
        guard offset + 8 <= limit else { return nil }
        let size32 = u32BE(bytes, at: offset)
        let kind = fourCC(bytes, at: offset + 4)
        if size32 == 1 {
            guard offset + 16 <= limit else { return nil }
            let size64 = u64BE(bytes, at: offset + 8)
            guard size64 >= 16, size64 <= UInt64(limit - offset) else { return nil }
            let boxEnd = offset + Int(size64)
            return Box(kind: kind, payloadStart: offset + 16, payloadEnd: boxEnd)
        }
        guard size32 >= 8, Int(size32) <= limit - offset else { return nil }
        let boxEnd = offset + Int(size32)
        return Box(kind: kind, payloadStart: offset + 8, payloadEnd: boxEnd)
    }

    private static func readSizedInt(_ bytes: [UInt8], at cursor: inout Int, end: Int, size: Int) -> Int? {
        guard cursor + size <= end else { return nil }
        defer { cursor += size }
        switch size {
        case 0:
            return 0
        case 4:
            return Int(u32BE(bytes, at: cursor))
        case 8:
            let raw = u64BE(bytes, at: cursor)
            guard raw <= UInt64(Int.max) else { return nil }
            return Int(raw)
        default:
            return nil
        }
    }

    private static func hasPrefix(_ bytes: [UInt8], at start: Int, end: Int, prefix: [UInt8]) -> Bool {
        guard start + prefix.count <= end else { return false }
        for index in 0..<prefix.count where bytes[start + index] != prefix[index] { return false }
        return true
    }

    private static func fourCC(_ bytes: [UInt8], at offset: Int) -> String {
        String(decoding: bytes[offset..<(offset + 4)], as: UTF8.self)
    }

    private static func u16BE(_ bytes: [UInt8], at offset: Int) -> UInt16 {
        (UInt16(bytes[offset]) << 8) | UInt16(bytes[offset + 1])
    }

    private static func u32BE(_ bytes: [UInt8], at offset: Int) -> UInt32 {
        (UInt32(bytes[offset]) << 24)
            | (UInt32(bytes[offset + 1]) << 16)
            | (UInt32(bytes[offset + 2]) << 8)
            | UInt32(bytes[offset + 3])
    }

    private static func u64BE(_ bytes: [UInt8], at offset: Int) -> UInt64 {
        (UInt64(u32BE(bytes, at: offset)) << 32) | UInt64(u32BE(bytes, at: offset + 4))
    }
}
