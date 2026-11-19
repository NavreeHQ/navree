import AVFoundation
import CoreMedia
import Foundation
import UniformTypeIdentifiers

/// The place a film was made, which a phone, a camera or a drone writes into
/// the container beside its video and sound tracks (`©xyz`, QuickTime's
/// `com.apple.quicktime.location.ISO6709`, 3GPP's `loci`, XMP, or a timed
/// metadata track).
///
/// Unless the field asks to keep it (`keep_location`), a picked film is
/// exported through the system's own exporter with sharing metadata filtering
/// on, and the written file is checked again through AVFoundation and by
/// walking its container boxes before it is kept. When export or verification
/// fails, nothing is kept: the film is never sent with its place.
enum FilmPlace {
    /// What became of a picked film as it was prepared for a form.
    enum Outcome: Equatable, Sendable {
        /// It was copied (when `keepingPlace`) or exported and verified clean.
        case copied
        /// The source file could not be read, or its copy could not be written.
        case unreadable
        /// The film could not be exported without what could say where it was made.
        case placeStays
    }

    typealias Exporter = @Sendable (URL, URL) async -> Bool

    static let systemExport: Exporter = { source, destination in
        await exportWithoutPlace(from: source, to: destination)
    }

    private static let filmExtensions: Set<String> = [
        "mov", "mp4", "m4v", "3gp", "3g2", "avi", "mkv", "webm",
        "mts", "m2ts", "ts", "mpg", "mpeg", "wmv", "flv", "ogv",
    ]

    private static let isoFilmBrands: Set<String> = [
        "qt  ", "isom", "iso2", "iso4", "iso5", "iso6",
        "mp41", "mp42", "M4V ", "M4VP", "3gp4", "3gp5", "3gp6", "3g2a",
        "avc1", "f4v ", "mmp4",
    ]

    /// Whether `url` is a film (video) by its extension or container magic,
    /// excluding still pictures handled by ``PhotoPlace``.
    static func isFilm(_ url: URL, originalExtension: String? = nil) -> Bool {
        let ext = (originalExtension ?? url.pathExtension).lowercased()
        if filmExtensions.contains(ext) { return true }
        if !ext.isEmpty, let named = UTType(filenameExtension: ext) {
            if named.conforms(to: .image) { return false }
            if named.conforms(to: .movie) || named.conforms(to: .video) { return true }
        }
        if PhotoPlace.isPicture(url, originalExtension: ext) { return false }
        guard let head = prefix(of: url, count: 32), head.count >= 12 else { return false }
        let bytes = [UInt8](head)
        // Matroska / WebM EBML header.
        if bytes.starts(with: [0x1A, 0x45, 0xDF, 0xA3]) { return true }
        // AVI RIFF container.
        if bytes.starts(with: [0x52, 0x49, 0x46, 0x46])
            && String(decoding: bytes[8..<12], as: UTF8.self) == "AVI "
        {
            return true
        }
        // QuickTime / ISO base media file format movie brands.
        let boxKind = String(decoding: bytes[4..<8], as: UTF8.self)
        if boxKind == "ftyp" {
            let brand = String(decoding: bytes[8..<12], as: UTF8.self)
            if isoFilmBrands.contains(brand) { return true }
        } else if boxKind == "moov" || boxKind == "wide" {
            return true
        }
        return false
    }

    /// Copies or exports the film at `source` to `copy`: byte for byte when
    /// `keepingPlace` is `true`, otherwise exported without location metadata
    /// and verified clean before it is kept at `copy`.
    static func copy(
        _ source: URL, originalExtension: String? = nil, to copy: URL,
        keepingPlace: Bool, exporter: Exporter = systemExport
    ) async -> Outcome {
        try? FileManager.default.removeItem(at: copy)
        if keepingPlace {
            return plain(source, to: copy) ? .copied : .unreadable
        }
        let ext = (originalExtension ?? source.pathExtension).lowercased()
        let stagedBase = FileManager.default.temporaryDirectory
            .appendingPathComponent("navree-film-in-\(UUID().uuidString)")
        let staged = ext.isEmpty ? stagedBase : stagedBase.appendingPathExtension(ext)
        guard plain(source, to: staged) else { return .unreadable }
        defer { try? FileManager.default.removeItem(at: staged) }

        let outBase = FileManager.default.temporaryDirectory
            .appendingPathComponent("navree-film-out-\(UUID().uuidString)")
        let outExt = preferredExtension(for: ext)
        let exported = outBase.appendingPathExtension(outExt)
        defer { try? FileManager.default.removeItem(at: exported) }

        guard await exporter(staged, exported), await isBare(exported) else {
            try? FileManager.default.removeItem(at: copy)
            return .placeStays
        }
        do {
            try FileManager.default.moveItem(at: exported, to: copy)
            return .copied
        } catch {
            try? FileManager.default.removeItem(at: copy)
            return .unreadable
        }
    }

    /// Exports `source` to `destination` without location or identifying
    /// metadata using ``AVAssetExportSession`` and ``AVMetadataItemFilter/forSharing()``.
    private static func exportWithoutPlace(from source: URL, to destination: URL) async -> Bool {
        let asset = AVURLAsset(url: source)
        guard let tracks = try? await asset.load(.tracks), !tracks.isEmpty else { return false }
        let hasOnlyMediaTracks = tracks.allSatisfy { $0.mediaType == .video || $0.mediaType == .audio }
        let fileTypes = preferredFileTypes(for: source.pathExtension.lowercased())

        if hasOnlyMediaTracks {
            for preset in [AVAssetExportPresetPassthrough, AVAssetExportPresetHighestQuality] {
                if await runExport(asset: asset, preset: preset, types: fileTypes, to: destination),
                    await isBare(destination)
                {
                    return true
                }
            }
        }

        // Rebuild from video and audio tracks only so timed metadata tracks
        // cannot carry coordinates into the exported file.
        let slices = await loadSlices(from: tracks)
        guard !slices.isEmpty else { return false }
        for preset in [AVAssetExportPresetPassthrough, AVAssetExportPresetHighestQuality] {
            if await runSliceExport(slices: slices, preset: preset, types: fileTypes, to: destination),
                await isBare(destination)
            {
                return true
            }
        }
        try? FileManager.default.removeItem(at: destination)
        return false
    }

    private struct TrackSlice: Sendable {
        let track: AVAssetTrack
        let mediaType: AVMediaType
        let range: CMTimeRange
        let transform: CGAffineTransform?
    }

    private static func loadSlices(from tracks: [AVAssetTrack]) async -> [TrackSlice] {
        var slices: [TrackSlice] = []
        for track in tracks where track.mediaType == .video || track.mediaType == .audio {
            guard let range = try? await track.load(.timeRange) else { continue }
            let transform: CGAffineTransform? =
                track.mediaType == .video ? (try? await track.load(.preferredTransform)) : nil
            slices.append(TrackSlice(track: track, mediaType: track.mediaType, range: range, transform: transform))
        }
        return slices
    }

    private static func runExport(
        asset: AVURLAsset, preset: String, types: [(AVFileType, String)], to destination: URL
    ) async -> Bool {
        for (fileType, ext) in types {
            if Task.isCancelled { return false }
            let attempt = FileManager.default.temporaryDirectory
                .appendingPathComponent("navree-film-try-\(UUID().uuidString)")
                .appendingPathExtension(ext)
            defer { try? FileManager.default.removeItem(at: attempt) }
            guard let session = AVAssetExportSession(asset: asset, presetName: preset) else { continue }
            session.metadata = []
            session.metadataItemFilter = AVMetadataItemFilter.forSharing()
            do {
                try await session.export(to: attempt, as: fileType)
                try? FileManager.default.removeItem(at: destination)
                try FileManager.default.moveItem(at: attempt, to: destination)
                return true
            } catch {
                continue
            }
        }
        return false
    }

    private static func runSliceExport(
        slices: [TrackSlice], preset: String, types: [(AVFileType, String)], to destination: URL
    ) async -> Bool {
        for (fileType, ext) in types {
            if Task.isCancelled { return false }
            let attempt = FileManager.default.temporaryDirectory
                .appendingPathComponent("navree-film-try-\(UUID().uuidString)")
                .appendingPathExtension(ext)
            defer { try? FileManager.default.removeItem(at: attempt) }
            guard let session = makeSliceSession(slices: slices, preset: preset) else { continue }
            do {
                try await session.export(to: attempt, as: fileType)
                try? FileManager.default.removeItem(at: destination)
                try FileManager.default.moveItem(at: attempt, to: destination)
                return true
            } catch {
                continue
            }
        }
        return false
    }

    private static func makeSliceSession(slices: [TrackSlice], preset: String) -> AVAssetExportSession? {
        let composition = AVMutableComposition()
        var added = 0
        for slice in slices {
            guard
                let target = composition.addMutableTrack(
                    withMediaType: slice.mediaType, preferredTrackID: kCMPersistentTrackID_Invalid
                )
            else { continue }
            do {
                try target.insertTimeRange(slice.range, of: slice.track, at: .zero)
                if let transform = slice.transform { target.preferredTransform = transform }
                added += 1
            } catch {
                continue
            }
        }
        guard added > 0, let session = AVAssetExportSession(asset: composition, presetName: preset) else {
            return nil
        }
        session.metadata = []
        session.metadataItemFilter = AVMetadataItemFilter.forSharing()
        return session
    }

    /// Whether the film at `url` reads cleanly as an audiovisual asset with
    /// zero location metadata items, zero timed metadata tracks, and clean
    /// container boxes.
    static func isBare(_ url: URL) async -> Bool {
        guard hasCleanBoxes(at: url) else { return false }
        let asset = AVURLAsset(url: url)
        guard let tracks = try? await asset.load(.tracks), !tracks.isEmpty else { return false }
        if tracks.contains(where: { $0.mediaType == .metadata }) { return false }

        guard let topMetadata = try? await asset.load(.metadata),
            let commonMetadata = try? await asset.load(.commonMetadata),
            let formats = try? await asset.load(.availableMetadataFormats)
        else { return false }
        if (topMetadata + commonMetadata).contains(where: { itemHoldsPlace($0) }) { return false }
        for format in formats {
            guard let items = try? await asset.loadMetadata(for: format) else { return false }
            if items.contains(where: { itemHoldsPlace($0) }) { return false }
        }

        for track in tracks {
            guard let trackMeta = try? await track.load(.metadata),
                let trackFormats = try? await track.load(.availableMetadataFormats)
            else { return false }
            if trackMeta.contains(where: { itemHoldsPlace($0) }) { return false }
            for format in trackFormats {
                guard let items = try? await track.loadMetadata(for: format) else { return false }
                if items.contains(where: { itemHoldsPlace($0) }) { return false }
            }
        }
        return true
    }

    private static func itemHoldsPlace(_ item: AVMetadataItem) -> Bool {
        if item.commonKey == .commonKeyLocation { return true }
        if let raw = item.identifier?.rawValue.lowercased() {
            if raw.contains("location") || raw.contains("gps") || raw.contains("xyz")
                || raw.contains("iso6709") || raw.hasSuffix("/loci")
            {
                return true
            }
        }
        return false
    }

    // MARK: - Container box inspection

    private static let xyzAtom: Data = Data([0xA9, 0x78, 0x79, 0x7A])
    private static let quickTimeLocationMark = Data("com.apple.quicktime.location".utf8)
    private static let iso6709Mark = Data("location.ISO6709".utf8)
    private static let containerBoxes: Set<String> = ["moov", "trak", "mdia", "minf", "udta"]
    private static let forbiddenBoxes: Set<String> = ["uuid", "XMP_", "xml ", "bxml", "loci"]

    /// Walks the ISOBMFF / QuickTime container at `url`, skipping `mdat`
    /// media frames and checking all header/metadata boxes for location atoms
    /// and XMP packets.
    static func hasCleanBoxes(at url: URL) -> Bool {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? handle.close() }
        guard let totalSize = try? handle.seekToEnd(), totalSize >= 16 else { return false }

        var cursor: UInt64 = 0
        var sawMoov = false
        var sawMdat = false

        while cursor < totalSize {
            guard cursor + 8 <= totalSize, (try? handle.seek(toOffset: cursor)) != nil,
                let header = try? handle.read(upToCount: 16), header.count >= 8
            else { return false }

            let headBytes = [UInt8](header)
            let size32 = u32BE(headBytes, at: 0)
            let kind = String(decoding: headBytes[4..<8], as: UTF8.self)
            let boxSize: UInt64
            let headerLen: UInt64
            if size32 == 1 {
                guard headBytes.count >= 16 else { return false }
                boxSize = u64BE(headBytes, at: 8)
                headerLen = 16
                guard boxSize >= 16 else { return false }
            } else if size32 == 0 {
                boxSize = totalSize - cursor
                headerLen = 8
                guard boxSize >= 8 else { return false }
            } else {
                boxSize = UInt64(size32)
                headerLen = 8
                guard boxSize >= 8 else { return false }
            }
            guard boxSize <= totalSize - cursor else { return false }
            if forbiddenBoxes.contains(kind) || headBytes[4..<8].elementsEqual([0xA9, 0x78, 0x79, 0x7A]) {
                return false
            }

            if kind == "mdat" {
                sawMdat = true
            } else {
                let payloadLen = boxSize - headerLen
                guard payloadLen <= 32 * 1024 * 1024,
                    (try? handle.seek(toOffset: cursor + headerLen)) != nil
                else { return false }
                let payload: Data
                if payloadLen == 0 {
                    payload = Data()
                } else {
                    guard let read = try? handle.read(upToCount: Int(payloadLen)),
                        read.count == Int(payloadLen)
                    else { return false }
                    payload = read
                }
                if kind == "free" || kind == "skip" {
                    if payload.contains(where: { $0 != 0 }) { return false }
                } else {
                    if PhotoPlace.holdsPacket(payload) { return false }
                    if payload.range(of: quickTimeLocationMark) != nil { return false }
                    if payload.range(of: iso6709Mark) != nil { return false }
                    if payload.range(of: xyzAtom) != nil { return false }
                    if kind == "moov" {
                        sawMoov = true
                        let moovBytes = [UInt8](payload)
                        if !walkBoxesClean(moovBytes, start: 0, end: moovBytes.count) { return false }
                    }
                }
            }
            cursor += boxSize
        }
        return sawMoov && sawMdat && cursor == totalSize
    }

    private static func walkBoxesClean(_ bytes: [UInt8], start: Int, end: Int) -> Bool {
        var cursor = start
        while cursor < end {
            // QuickTime user-data lists may end with a 32-bit zero terminator.
            if cursor + 4 <= end, u32BE(bytes, at: cursor) == 0, bytes[cursor..<end].allSatisfy({ $0 == 0 }) {
                return true
            }
            guard cursor + 8 <= end else { return false }
            let size32 = u32BE(bytes, at: cursor)
            let rawType = Array(bytes[(cursor + 4)..<(cursor + 8)])
            let kind = String(decoding: rawType, as: UTF8.self)
            let boxEnd: Int
            let payloadStart: Int
            if size32 == 1 {
                guard cursor + 16 <= end else { return false }
                let size64 = u64BE(bytes, at: cursor + 8)
                guard size64 >= 16, size64 <= UInt64(end - cursor) else { return false }
                payloadStart = cursor + 16
                boxEnd = cursor + Int(size64)
            } else if size32 == 0 {
                payloadStart = cursor + 8
                boxEnd = end
            } else {
                guard size32 >= 8, Int(size32) <= end - cursor else { return false }
                payloadStart = cursor + 8
                boxEnd = cursor + Int(size32)
            }
            if rawType == [0xA9, 0x78, 0x79, 0x7A] || forbiddenBoxes.contains(kind) { return false }
            if containerBoxes.contains(kind) {
                if !walkBoxesClean(bytes, start: payloadStart, end: boxEnd) { return false }
            }
            cursor = boxEnd
        }
        return cursor == end
    }

    // MARK: - Helpers

    private static func preferredExtension(for ext: String) -> String {
        switch ext {
        case "mp4": "mp4"
        case "m4v": "m4v"
        default: "mov"
        }
    }

    private static func preferredFileTypes(for ext: String) -> [(AVFileType, String)] {
        switch ext {
        case "mp4": [(.mp4, "mp4"), (.mov, "mov")]
        case "m4v": [(.m4v, "m4v"), (.mov, "mov"), (.mp4, "mp4")]
        default: [(.mov, "mov"), (.mp4, "mp4")]
        }
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
