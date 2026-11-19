@preconcurrency import AVFoundation
import CoreGraphics
import CoreMedia
import CoreVideo
import Darwin
import Foundation
import ImageIO
import NavreeDesign
import NavreeProtocol
import NavreeRuntime
import Synchronization
import Testing
import UniformTypeIdentifiers

@testable import NavreeShell

private nonisolated func runsOnMainThread() -> Bool {
    pthread_main_np() != 0
}

/// Whether this system can write a HEIC picture: the test images are made in code.
private let writesHEIC: Bool = {
    let kinds = CGImageDestinationCopyTypeIdentifiers() as? [String] ?? []
    return kinds.contains(UTType.heic.identifier)
}()

/// What could say where a photo was taken is taken out of it before a form
/// sends it, unless the field asks for the place.
@MainActor
struct PhotoPlaceTests {
    /// A folder of the test's own.
    static func folder() throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-photos-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// Where the Golden Temple in Amritsar is, as a camera writes it.
    static var place: [CFString: Any] {
        [
            kCGImagePropertyGPSLatitude: 31.62, kCGImagePropertyGPSLatitudeRef: "N",
            kCGImagePropertyGPSLongitude: 74.8765, kCGImagePropertyGPSLongitudeRef: "E",
        ]
    }

    /// A small picture of one colour, written as `kind` with `properties` beside its pixels.
    static func picture(as kind: UTType = .jpeg, properties: [CFString: Any] = [:]) throws -> Data {
        let context = try #require(CGContext(
            data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
        ))
        context.setFillColor(red: 0.95, green: 0.6, blue: 0.1, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: 8, height: 8))
        let image = try #require(context.makeImage())
        let data = NSMutableData()
        let destination = try #require(
            CGImageDestinationCreateWithData(data as CFMutableData, kind.identifier as CFString, 1, nil)
        )
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        let written = CGImageDestinationFinalize(destination)
        #expect(written)
        return data as Data
    }

    /// An XMP packet as a drone and a caption tool write one: the maker's
    /// own fields for where it flew, and a place inside a structure.
    static let packet = """
        <?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
        <x:xmpmeta xmlns:x="adobe:ns:meta/">
         <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
          <rdf:Description rdf:about=""
            xmlns:drone-dji="http://www.dji.com/drone-dji/1.0/"
            xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/"
            xmlns:exif="http://ns.adobe.com/exif/1.0/"
            drone-dji:GpsLatitude="31.6200" drone-dji:GpsLongitude="74.8765">
           <Iptc4xmpExt:LocationCreated>
            <rdf:Bag>
             <rdf:li rdf:parseType="Resource">
              <Iptc4xmpExt:City>Amritsar</Iptc4xmpExt:City>
              <exif:GPSLatitude>31,37.2N</exif:GPSLatitude>
              <exif:GPSLongitude>74,52.59E</exif:GPSLongitude>
             </rdf:li>
            </rdf:Bag>
           </Iptc4xmpExt:LocationCreated>
          </rdf:Description>
         </rdf:RDF>
        </x:xmpmeta>
        <?xpacket end="w"?>
        """

    /// What of the packet must never be in what a form sends.
    static let told = ["drone-dji", "GpsLatitude", "LocationCreated", "Amritsar", "31,37.2N", "x:xmpmeta"]

    /// A JPEG with an XMP packet put in after its first segment, in the
    /// segment a camera writes one in.
    static func jpeg(_ jpeg: Data, with packet: String) -> Data {
        let bytes = [UInt8](jpeg)
        // The start of the picture (2 bytes), then a segment: its marker (2) and its length, which counts itself.
        let first = 4 + (Int(bytes[4]) << 8 | Int(bytes[5]))
        let payload = [UInt8]("http://ns.adobe.com/xap/1.0/\u{0}".utf8) + [UInt8](packet.utf8)
        let length = payload.count + 2
        var out = Array(bytes[..<first])
        out += [0xFF, 0xE1, UInt8(length >> 8), UInt8(length & 0xFF)]
        out += payload
        out += bytes[first...]
        return Data(out)
    }

    /// A PNG with an XMP packet put in after its header, in the text chunk PNG keeps one in.
    static func png(_ png: Data, with packet: String) -> Data {
        let bytes = [UInt8](png)
        // The chunk's kind, its keyword and the keyword's end, not
        // compressed (2 bytes), no language, no translated keyword, the text.
        var chunk = [UInt8]("iTXt".utf8)
        chunk += [UInt8]("XML:com.adobe.xmp".utf8)
        chunk += [UInt8](repeating: 0, count: 5)
        chunk += [UInt8](packet.utf8)
        // The signature and the header chunk: 8 + 4 + 4 + 13 + 4 bytes.
        var out = Array(bytes[..<33])
        out += word(UInt32(chunk.count - 4))
        out += chunk
        out += word(checksum(chunk))
        out += bytes[33...]
        return Data(out)
    }

    /// A number as PNG writes one: four bytes, the largest first.
    static func word(_ value: UInt32) -> [UInt8] {
        [UInt8(value >> 24), UInt8((value >> 16) & 0xFF), UInt8((value >> 8) & 0xFF), UInt8(value & 0xFF)]
    }

    /// The checksum a PNG chunk ends with (CRC-32).
    static func checksum(_ bytes: [UInt8]) -> UInt32 {
        var crc: UInt32 = 0xFFFF_FFFF
        for byte in bytes {
            crc ^= UInt32(byte)
            for _ in 0..<8 {
                crc = (crc & 1) == 1 ? (crc >> 1) ^ 0xEDB8_8320 : crc >> 1
            }
        }
        return crc ^ 0xFFFF_FFFF
    }

    /// The groups of properties the first image of a picture has, by name.
    static func groups(of picture: Data) throws -> Set<String> {
        let source = try #require(CGImageSourceCreateWithData(picture as CFData, nil))
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] ?? [:]
        return Set(properties.keys)
    }

    /// What the camera's owner wrote into the first image of a picture, when there is such a field.
    static func ownersWords(of picture: Data) throws -> String? {
        let source = try #require(CGImageSourceCreateWithData(picture as CFData, nil))
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        let exif = properties?[kCGImagePropertyExifDictionary as String] as? [String: Any]
        return exif?[kCGImagePropertyExifUserComment as String] as? String
    }

    /// The kind of picture some bytes are, and how wide its first image is.
    static func shape(of picture: Data) throws -> (kind: String, width: Int?) {
        let source = try #require(CGImageSourceCreateWithData(picture as CFData, nil))
        let kind = try #require(CGImageSourceGetType(source)) as String
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        return (kind, properties?[kCGImagePropertyPixelWidth as String] as? Int)
    }

    static func holds(_ data: Data, _ words: String) -> Bool {
        data.range(of: Data(words.utf8)) != nil
    }

    /// Writes `data` as a file named `name` in `folder`, keeps it for a form
    /// in a store of the test's own, and gives what the form would send.
    static func sent(_ data: Data, named name: String, in folder: URL, keepLocation: Bool = false) throws -> (file: FileRef, bytes: Data) {
        let url = folder.appendingPathComponent(name)
        try data.write(to: url)
        let store = FileStore(folder: folder.appendingPathComponent("kept-\(UUID().uuidString)"))
        guard case .kept(let file) = store.take(url, keepLocation: keepLocation) else {
            throw Refused(name: name)
        }
        return (file, try #require(store.data(file.token)))
    }

    struct Refused: Error {
        let name: String
    }

    // MARK: - The place

    @Test func thePlaceIsTakenOutOfAPhotoBeforeItIsKeptForAForm() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = try Self.picture(properties: [kCGImagePropertyGPSDictionary: Self.place])
        let before = try Self.groups(of: photo)
        #expect(before.contains("{GPS}"), "the photo the test made says where it was taken")
        #expect(!PhotoPlace.isBare(photo))

        let sent = try Self.sent(photo, named: "receipt.jpg", in: folder)
        #expect(sent.file.name == "receipt.jpg")
        #expect(sent.file.type == "image/jpeg")
        #expect(sent.file.size == sent.bytes.count)
        // What a form sends is still the picture, and says nothing of where it was taken.
        #expect(sent.bytes != photo)
        #expect(PhotoPlace.isBare(sent.bytes))
        let after = try Self.groups(of: sent.bytes)
        #expect(!after.contains("{GPS}"))
        let shape = try Self.shape(of: sent.bytes)
        #expect(shape.kind == UTType.jpeg.identifier)
        #expect(shape.width == 8)
    }

    @Test func aFieldThatAsksForThePlaceIsSentThePhotoAsItIs() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = Self.jpeg(try Self.picture(properties: [kCGImagePropertyGPSDictionary: Self.place]), with: Self.packet)
        let sent = try Self.sent(photo, named: "site.jpg", in: folder, keepLocation: true)
        #expect(sent.bytes == photo)
    }

    @Test func aPhotoThatHoldsNothingOfThatAndAFileThatIsNoPictureAreKeptByteForByte() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = try Self.picture()
        #expect(PhotoPlace.isBare(photo))
        #expect(try Self.sent(photo, named: "label.jpg", in: folder).bytes == photo)

        let notes = Data("Wrap the dupattas in tissue".utf8)
        let file = folder.appendingPathComponent("notes.txt")
        try notes.write(to: file)
        #expect(!PhotoPlace.isPicture(file))
        #expect(try Self.sent(notes, named: "notes.txt", in: folder).bytes == notes)
    }

    @Test func aPictureIsKnownByWhatIsInItWhateverItsName() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        // A photo sent under a name that says nothing of what it is.
        let photo = try Self.picture(properties: [kCGImagePropertyGPSDictionary: Self.place])
        let file = folder.appendingPathComponent("upload.bin")
        try photo.write(to: file)
        #expect(PhotoPlace.isPicture(file))
        let sent = try Self.sent(photo, named: "upload.bin", in: folder)
        #expect(PhotoPlace.isBare(sent.bytes))
    }

    // MARK: - The other ways a picture says where it was taken

    @Test func aPlaceInsideThePacketAndAMakersOwnFieldsGoWithTheWholePacket() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        // No GPS data at all: the place is in the XMP packet only, under a
        // drone maker's own names and inside a structure.
        let photo = Self.jpeg(try Self.picture(), with: Self.packet)
        #expect(PhotoPlace.holdsPacket(photo))
        #expect(!PhotoPlace.isBare(photo))
        #expect(try Self.shape(of: photo).width == 8, "the photo the test made still reads as a picture")

        let sent = try Self.sent(photo, named: "survey.jpg", in: folder)
        #expect(PhotoPlace.isBare(sent.bytes))
        #expect(!PhotoPlace.holdsPacket(sent.bytes))
        for words in Self.told {
            #expect(!Self.holds(sent.bytes, words), "\(words)")
        }
        let shape = try Self.shape(of: sent.bytes)
        #expect(shape.kind == UTType.jpeg.identifier)
        #expect(shape.width == 8)
    }

    @Test func aMakersNotesAndTheOwnersWordsGoToo() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = try Self.picture(properties: [
            kCGImagePropertyMakerAppleDictionary: ["17": "1F0C6A2E-77AB-4D21-9A04-5D21C0FFEE00"],
            kCGImagePropertyExifDictionary: [
                kCGImagePropertyExifUserComment: "Taken outside the shop in Hall Bazaar",
                kCGImagePropertyExifDateTimeOriginal: "2026:11:12 09:41:00",
            ] as [CFString: Any],
            kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "Charkha", kCGImagePropertyTIFFModel: "Field camera"],
        ])
        let before = try Self.groups(of: photo)
        #expect(before.contains("{MakerApple}"), "the photo the test made holds a maker's notes")
        #expect(try Self.ownersWords(of: photo) == "Taken outside the shop in Hall Bazaar", "and the owner's own words")
        #expect(!PhotoPlace.isBare(photo))

        let sent = try Self.sent(photo, named: "counter.jpg", in: folder)
        #expect(PhotoPlace.isBare(sent.bytes))
        let after = try Self.groups(of: sent.bytes)
        let makers = after.filter { $0.hasPrefix("{Maker") }
        #expect(makers.isEmpty, "\(makers)")
        #expect(try Self.ownersWords(of: sent.bytes) == nil)
        #expect(!Self.holds(sent.bytes, "Hall Bazaar"))
        #expect(!Self.holds(sent.bytes, "1F0C6A2E"))
        #expect(try Self.shape(of: sent.bytes).width == 8)
    }

    @Test func aPNGsPacketGoesAsAJPEGsDoes() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let plain = try Self.picture(as: .png)
        #expect(PhotoPlace.isBare(plain))
        let photo = Self.png(plain, with: Self.packet)
        #expect(PhotoPlace.holdsPacket(photo))
        #expect(!PhotoPlace.isBare(photo))
        let made = try Self.shape(of: photo)
        #expect(made.kind == UTType.png.identifier, "the picture the test made still reads as a PNG")
        #expect(made.width == 8)

        let sent = try Self.sent(photo, named: "screen.png", in: folder)
        #expect(sent.file.type == "image/png")
        #expect(PhotoPlace.isBare(sent.bytes))
        for words in Self.told + ["XML:com.adobe.xmp"] {
            #expect(!Self.holds(sent.bytes, words), "\(words)")
        }
        let shape = try Self.shape(of: sent.bytes)
        #expect(shape.kind == UTType.png.identifier)
        #expect(shape.width == 8)
    }

    @Test(.enabled(if: writesHEIC))
    func aPhoneCamerasPictureLosesItsPlaceToo() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = try Self.picture(as: .heic, properties: [kCGImagePropertyGPSDictionary: Self.place])
        let before = try Self.groups(of: photo)
        #expect(before.contains("{GPS}"), "the photo the test made says where it was taken")
        #expect(!PhotoPlace.isBare(photo))

        let sent = try Self.sent(photo, named: "IMG_2048.HEIC", in: folder)
        #expect(PhotoPlace.isBare(sent.bytes))
        let after = try Self.groups(of: sent.bytes)
        #expect(!after.contains("{GPS}"))
        let shape = try Self.shape(of: sent.bytes)
        #expect(shape.kind == UTType.heic.identifier)
        #expect(shape.width == 8)
    }

    // MARK: - A file that is not added

    @Test func aPictureThatCannotBeLookedIntoIsNotKeptAndSaysWhy() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        // A picture by its name, and nothing that reads as one: whether it
        // says where it was taken cannot be told, so it is not sent as it is.
        let broken = folder.appendingPathComponent("holiday.jpg")
        try Data("not a picture".utf8).write(to: broken)
        let kept = folder.appendingPathComponent("kept")
        let store = FileStore(folder: kept)
        #expect(store.take(broken) == .refused(.placeUnchecked))
        #expect(store.keep(broken) == nil)
        // Nothing of it is left among the kept files.
        let left = (try? FileManager.default.contentsOfDirectory(atPath: kept.path)) ?? []
        #expect(left.isEmpty)
        // A file that is not there could not be read.
        #expect(store.take(folder.appendingPathComponent("gone.pdf")) == .refused(.unreadable))
        // A field that asks for the place takes the file as it is.
        #expect(store.keep(broken, keepLocation: true) != nil)
    }

    @Test func aFileThatIsNotAddedIsNamedAndSaidWhyInWords() {
        #expect(Words.refused("gone.pdf", .unreadable) == "gone.pdf could not be read and was not added.")
        #expect(
            Words.refused("holiday.jpg", .placeUnchecked)
                == "holiday.jpg is a kind of picture this device cannot check for the place it was taken, so it was not added."
        )
        #expect(
            Words.refused("Kapurthala.dng", .placeStays)
                == "Kapurthala.dng cannot be sent in this format without details that could say where it was taken, so it was not added. Save it as JPEG or PNG and pick it again."
        )
        #expect(
            Words.refused("walkthrough.mov", .filmPlaceStays)
                == "walkthrough.mov could not be prepared without the place it was made, so it was not added."
        )
    }

    // MARK: - Container bytes, properties and fallback

    @Test func aPictureWhosePropertiesDoNotReadIsNeverSentAsItIs() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }

        #expect(!PhotoPlace.arePropertiesBare(nil))
        #expect(!PhotoPlace.arePropertiesBare([:]))

        // A clean picture whose properties fail to read is never copied byte for byte:
        // when rewritten copies do read cleanly, it is rewritten; when properties never
        // read at all, it is refused rather than sent as it was.
        let clean = try Self.picture()
        let sourceURL = folder.appendingPathComponent("counter.jpg")
        try clean.write(to: sourceURL)
        #expect(!PhotoPlace.isBare(clean, readableProperties: false))

        let rewrittenURL = folder.appendingPathComponent("rewritten.jpg")
        #expect(
            PhotoPlace.copy(sourceURL, to: rewrittenURL, keepingPlace: false, propertyReading: .unreadableOriginal)
                == .copied
        )
        let rewritten = try Data(contentsOf: rewrittenURL)
        #expect(PhotoPlace.isBare(rewritten))

        let refusedURL = folder.appendingPathComponent("refused.jpg")
        #expect(
            PhotoPlace.copy(sourceURL, to: refusedURL, keepingPlace: false, propertyReading: .alwaysUnreadable)
                == .placeStays
        )
        #expect(!FileManager.default.fileExists(atPath: refusedURL.path))

        // A JPEG carrying an Exif segment with broken IFD offsets — which Image I/O
        // skips while another tool can still read coordinates from its bytes — is
        // rejected by the container check and rewritten without that segment.
        let brokenExif = Self.jpegWithBrokenExif(clean, hiddenText: "Amritsar 31.6200,74.8765")
        #expect(Self.holds(brokenExif, "Amritsar"))
        #expect(!PhotoPlace.isBare(brokenExif))
        let sent = try Self.sent(brokenExif, named: "broken-exif.jpg", in: folder)
        #expect(sent.bytes != brokenExif)
        #expect(PhotoPlace.isBare(sent.bytes))
        #expect(!Self.holds(sent.bytes, "Amritsar"))
    }

    @Test func aPngWithAProfileChunkIsWrittenAgainWithoutIt() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let plain = try Self.picture(as: .png)
        #expect(PhotoPlace.isBare(plain))

        // ImageMagick-style compressed `zTXt` chunk named `Raw profile type exif`
        // holding hex-encoded Exif GPS coordinates of Amritsar: Image I/O reports no
        // `{GPS}` group and no XMP mark appears in the raw bytes.
        let profiled = try Self.pngWithProfileChunk(plain)
        #expect(try !Self.groups(of: profiled).contains("{GPS}"))
        #expect(!PhotoPlace.holdsPacket(profiled))
        #expect(Self.holds(profiled, "Raw profile type exif"))
        #expect(!PhotoPlace.isBare(profiled))

        let sent = try Self.sent(profiled, named: "site-plan.png", in: folder)
        #expect(sent.bytes != profiled)
        #expect(PhotoPlace.isBare(sent.bytes))
        #expect(!Self.holds(sent.bytes, "Raw profile type exif"))
        #expect(!Self.holds(sent.bytes, "zTXt"))
        let shape = try Self.shape(of: sent.bytes)
        #expect(shape.kind == UTType.png.identifier)
        #expect(shape.width == 8)
    }

    @Test func bytesAfterThePicturesEndAreNotSent() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let base = try Self.picture()
        #expect(PhotoPlace.isBare(base))

        // 1. Maker trailer after EOI carrying India's mobile country code (MCC 404).
        let trailed = Self.jpegWithMakerTrailer(base)
        #expect(try !Self.groups(of: trailed).contains("{GPS}"))
        #expect(!PhotoPlace.holdsPacket(trailed))
        #expect(!PhotoPlace.isBare(trailed))

        let sentTrailed = try Self.sent(trailed, named: "IMG_20261118.jpg", in: folder)
        #expect(PhotoPlace.isBare(sentTrailed.bytes))
        #expect(sentTrailed.bytes.suffix(2) == Data([0xFF, 0xD9]))
        #expect(!Self.holds(sentTrailed.bytes, "MCC_Data"))
        #expect(!Self.holds(sentTrailed.bytes, "SEFT"))

        // 2. Motion photo: a clean JPEG followed after EOI by an MP4 container carrying
        // QuickTime `©xyz` coordinates for Amritsar, with no XMP mark in the JPEG.
        let motion = Self.motionPhoto(base)
        #expect(try !Self.groups(of: motion).contains("{GPS}"))
        #expect(!PhotoPlace.holdsPacket(motion))
        #expect(!PhotoPlace.isBare(motion))

        let sentMotion = try Self.sent(motion, named: "MP_20261118.jpg", in: folder)
        #expect(PhotoPlace.isBare(sentMotion.bytes))
        #expect(sentMotion.bytes.suffix(2) == Data([0xFF, 0xD9]))
        #expect(!Self.holds(sentMotion.bytes, "+31.6200+074.8765"))
        #expect(!Self.holds(sentMotion.bytes, "ftyp"))
        #expect(try Self.shape(of: sentMotion.bytes).width == 8)
    }

    @Test func whatTheFirstWayWroteIsCheckedAgainAndTheSecondWayIsUsedWhereItFails() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let photo = try Self.picture(properties: [kCGImagePropertyGPSDictionary: Self.place])
        let sourceURL = folder.appendingPathComponent("depot.jpg")
        try photo.write(to: sourceURL)
        let copyURL = folder.appendingPathComponent("clean.jpg")

        let cleanSample = Self.jpegWithMakerTrailer(try Self.picture())
        let calls = Mutex<[Bool]>([])
        let outcome = PhotoPlace.copy(
            sourceURL, to: copyURL, keepingPlace: false,
            rewriter: { sourceBytes, destination, pixelsUntouched in
                calls.withLock { $0.append(pixelsUntouched) }
                if pixelsUntouched {
                    // Simulate a lossless first pass that left maker bytes after EOI.
                    return (try? cleanSample.write(to: destination)) != nil
                }
                return PhotoPlace.defaultRewriter(sourceBytes, destination, false)
            }
        )
        #expect(outcome == .copied)
        #expect(calls.withLock { $0 } == [true, false])
        let written = try Data(contentsOf: copyURL)
        #expect(PhotoPlace.isBare(written))
        #expect(!Self.holds(written, "MCC_Data"))
        #expect(try Self.shape(of: written).width == 8)

        // When both passes leave unclean bytes, nothing is kept at all.
        let failedURL = folder.appendingPathComponent("failed.jpg")
        let failedOutcome = PhotoPlace.copy(
            sourceURL, to: failedURL, keepingPlace: false,
            rewriter: { _, destination, _ in
                (try? cleanSample.write(to: destination)) != nil
            }
        )
        #expect(failedOutcome == .placeStays)
        #expect(!FileManager.default.fileExists(atPath: failedURL.path))
    }

    @Test func aDesignFileOrARawIsRefusedWithWordsThatSayWhatToDo() async throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let store = FileStore(folder: folder.appendingPathComponent("kept"))

        let svg = folder.appendingPathComponent("emblem.svg")
        try Data("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>".utf8).write(to: svg)

        // Minimal Photoshop 8BPS header.
        var psdBytes: [UInt8] = [0x38, 0x42, 0x50, 0x53, 0x00, 0x01]
        psdBytes += [UInt8](repeating: 0, count: 20)
        let psd = folder.appendingPathComponent("banner.psd")
        try Data(psdBytes).write(to: psd)

        let dng = folder.appendingPathComponent("Amritsar.dng")
        try Self.picture(as: .tiff, properties: [kCGImagePropertyGPSDictionary: Self.place]).write(to: dng)

        let cr2 = folder.appendingPathComponent("Ludhiana.cr2")
        try Data([0x49, 0x49, 0x2A, 0x00, 0x10, 0x00, 0x00, 0x00, 0x43, 0x52, 0x02, 0x00]).write(to: cr2)

        let gif = folder.appendingPathComponent("badge.gif")
        try Self.picture(as: .gif).write(to: gif)

        let tiff = folder.appendingPathComponent("scan.tiff")
        try Self.picture(as: .tiff).write(to: tiff)

        for candidate in [svg, psd, dng, cr2, gif, tiff] {
            let picked = await store.takeAsync(candidate)
            #expect(picked == .refused(.placeStays), "\(candidate.lastPathComponent)")
            let message = Words.refused(candidate.lastPathComponent, .placeStays)
            #expect(message.contains(candidate.lastPathComponent))
            #expect(message.contains("Save it as JPEG or PNG and pick it again."))
            // When the field explicitly sets `keep_location`, the file is kept as it is.
            let allowed = await store.keepAsync(candidate, keepLocation: true)
            #expect(allowed != nil, "\(candidate.lastPathComponent)")
        }
    }

    @Test func photosAreWrittenAgainOffTheMainActorAndTheFieldShowsItIsBusy() async throws {
        let shell = try Shell()
        shell.transport.pages["/orders"] = """
            {"build": "152bd84abf4d", "navree": 1, "vocab": 0, "screen": {"title": "Site visit", "body":
              {"t": "form", "id": "visit", "method": "post", "action": "/visits", "children": [
                {"t": "field", "name": "photos", "label": "Site photos",
                 "input": {"t": "file", "accept": "image/*", "multiple": true}}]}}}
            """
        shell.client.begin()
        shell.client.send(.select(destination: 1))
        let screen = try #require(shell.client.nav.front)
        let tree = try #require(shell.client.screen(screen)?.screen)
        var foundField: Navree.Field?
        var foundForm: Navree.Form?
        tree.body.walk { node in
            if case .field(let f) = node { foundField = f }
            if case .form(let f) = node { foundForm = f }
        }
        let form = FormRef(try #require(foundForm))
        let field = try #require(foundField).key
        let context = shell.client.context(for: screen, depth: 0, base: nil)

        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photoURL = folder.appendingPathComponent("amritsar.jpg")
        try Self.picture(properties: [kCGImagePropertyGPSDictionary: Self.place]).write(to: photoURL)

        let (enteredStream, enteredContinuation) = AsyncStream<Void>.makeStream()
        let (releaseStream, releaseContinuation) = AsyncStream<Void>.makeStream()
        let workerOnMain = Mutex<Bool?>(nil)
        shell.effects.keeper.files.workObserver = {
            workerOnMain.withLock { $0 = runsOnMainThread() }
            enteredContinuation.yield()
            enteredContinuation.finish()
            for await _ in releaseStream { break }
        }
        defer { shell.effects.keeper.files.workObserver = nil }

        #expect(!context.fileBusy(form, field))
        context.send(.files(form: form, field: field, urls: [photoURL]))
        #expect(context.fileBusy(form, field))
        #expect(context.values(form, field) == nil)

        for await _ in enteredStream { break }
        #expect(workerOnMain.withLock { $0 } == false)
        #expect(context.fileBusy(form, field))

        releaseContinuation.finish()
        await shell.effects.fileNotes.settled()

        #expect(!context.fileBusy(form, field))
        #expect(context.values(form, field) == ["amritsar.jpg"])
        if case .files(let refs)? = shell.client.screen(screen)?.forms[form.key]?.fields[field]?.value {
            let ref = try #require(refs.first)
            let bytes = try #require(shell.effects.keeper.files.data(ref.token))
            #expect(PhotoPlace.isBare(bytes))
        } else {
            Issue.record("the rewritten photo did not reach the core")
        }
    }

    // MARK: - Films

    @Test func aFilmIsSentWithoutThePlaceItWasMadeAndNotSentWhenThatCannotBeDone() async throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let store = FileStore(folder: folder.appendingPathComponent("kept"))

        let clip = try await Self.film(in: folder, named: "hall-bazaar.mov", withPlace: true)
        let originalBytes = try Data(contentsOf: clip)
        #expect(FilmPlace.isFilm(clip))
        #expect(!PhotoPlace.isPicture(clip))
        #expect(Self.holds(originalBytes, "+31.6200+074.8765"))
        #expect(!(await FilmPlace.isBare(clip)))

        // An extensionless camera file with a vendor major brand and `mp42` in compatible brands is recognised as a film.
        let branded = folder.appendingPathComponent("camera-upload.bin")
        var ftypPayload = [UInt8]("ZZZZ".utf8)
        ftypPayload += [0x00, 0x00, 0x00, 0x00]
        ftypPayload += [UInt8]("ZZZZmp42".utf8)
        try Data(Self.box("ftyp", payload: ftypPayload)).write(to: branded)
        #expect(FilmPlace.isFilm(branded))

        guard case .kept(let sent) = await store.takeAsync(clip) else {
            Issue.record("the geotagged film should be exported without its place")
            return
        }
        let exportedBytes = try #require(store.data(sent.token))
        #expect(sent.name == "hall-bazaar.mov")
        #expect(sent.size == exportedBytes.count)
        #expect(!Self.holds(exportedBytes, "+31.6200+074.8765"))
        #expect(!Self.holds(exportedBytes, "location.ISO6709"))
        let checkURL = folder.appendingPathComponent("checked.mov")
        try exportedBytes.write(to: checkURL)
        #expect(await FilmPlace.isBare(checkURL))
        let tracks = try await AVURLAsset(url: checkURL).load(.tracks)
        #expect(!tracks.isEmpty)

        // When export fails (a broken movie container), the film is refused rather than sent.
        let broken = folder.appendingPathComponent("broken.mov")
        try Data("not a valid movie".utf8).write(to: broken)
        #expect(await store.takeAsync(broken) == .refused(.filmPlaceStays))

        // When an exporter leaves the place in the output, read-back verification refuses it too.
        let unstripped = await store.takeAsync(
            clip,
            filmExporter: { src, dst in
                (try? FileManager.default.copyItem(at: src, to: dst)) != nil
            }
        )
        #expect(unstripped == .refused(.filmPlaceStays))
    }

    @Test func aFieldThatAsksForThePlaceIsSentTheFilmAsItIs() async throws {
        let shell = try Shell()
        shell.transport.pages["/orders"] = """
            {"build": "152bd84abf4d", "navree": 1, "vocab": 0, "screen": {"title": "Field inspection", "body":
              {"t": "form", "id": "inspection", "method": "post", "action": "/inspections", "children": [
                {"t": "field", "name": "clip", "label": "Inspection video",
                 "input": {"t": "file", "accept": "video/*", "keep_location": true}}]}}}
            """
        shell.client.begin()
        shell.client.send(.select(destination: 1))
        let screen = try #require(shell.client.nav.front)
        let tree = try #require(shell.client.screen(screen)?.screen)
        var foundField: Navree.Field?
        var foundForm: Navree.Form?
        tree.body.walk { node in
            if case .field(let f) = node { foundField = f }
            if case .form(let f) = node { foundForm = f }
        }
        let form = FormRef(try #require(foundForm))
        let field = try #require(foundField)
        let context = shell.client.context(for: screen, depth: 0, base: nil)

        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let clip = try await Self.film(in: folder, named: "pune-depot.mov", withPlace: true)
        let original = try Data(contentsOf: clip)

        context.send(.files(form: form, field: field.key, urls: [clip], keepLocation: true))
        await shell.effects.fileNotes.settled()

        #expect(context.values(form, field.key) == ["pune-depot.mov"])
        #expect(context.notes(form, field.key).isEmpty)
        if case .files(let refs)? = shell.client.screen(screen)?.forms[form.key]?.fields[field.key]?.value {
            let ref = try #require(refs.first)
            let keptBytes = try #require(shell.effects.keeper.files.data(ref.token))
            #expect(keptBytes == original)
            #expect(Self.holds(keptBytes, "+31.6200+074.8765"))
        } else {
            Issue.record("the film was not kept as it is for a keep_location field")
        }
    }

    // MARK: - Fixture builders

    /// Inserts a JPEG `APP1` Exif segment whose TIFF root IFD offset points past
    /// the segment end, followed by hidden text bytes.
    static func jpegWithBrokenExif(_ jpeg: Data, hiddenText: String) -> Data {
        let bytes = [UInt8](jpeg)
        let first = 4 + (Int(bytes[4]) << 8 | Int(bytes[5]))
        var payload: [UInt8] = [0x45, 0x78, 0x69, 0x66, 0x00, 0x00]
        // Little-endian TIFF header with an out-of-range IFD0 offset (0x7FFF_FFFF).
        payload += [0x49, 0x49, 0x2A, 0x00, 0xFF, 0xFF, 0xFF, 0x7F]
        payload += [UInt8](hiddenText.utf8)
        let length = payload.count + 2
        var out = Array(bytes[..<first])
        out += [0xFF, 0xE1, UInt8(length >> 8), UInt8(length & 0xFF)]
        out += payload
        out += bytes[first...]
        return Data(out)
    }

    /// Inserts a compressed PNG `zTXt` chunk with keyword `Raw profile type exif`
    /// carrying hex-encoded Exif GPS bytes.
    static func pngWithProfileChunk(_ png: Data) throws -> Data {
        let exifHex = Data("Exif\0\0Amritsar 31.6200N 74.8765E".utf8)
            .map { String(format: "%02x", $0) }
            .joined()
        let profileText = "\nexif\n      \(exifHex.count / 2)\n\(exifHex)\n"
        let rawBytes = [UInt8](profileText.utf8)
        let deflated = try (Data(rawBytes) as NSData).compressed(using: .zlib) as Data
        var zlibStream: [UInt8] = [0x78, 0x9C]
        zlibStream += [UInt8](deflated)
        zlibStream += word(adler32(rawBytes))

        var chunk = [UInt8]("zTXt".utf8)
        chunk += [UInt8]("Raw profile type exif".utf8)
        chunk += [0x00, 0x00]
        chunk += zlibStream

        let bytes = [UInt8](png)
        var out = Array(bytes[..<33])
        out += word(UInt32(chunk.count - 4))
        out += chunk
        out += word(checksum(chunk))
        out += bytes[33...]
        return Data(out)
    }

    static func adler32(_ bytes: [UInt8]) -> UInt32 {
        var sumLow: UInt32 = 1
        var sumHigh: UInt32 = 0
        for byte in bytes {
            sumLow = (sumLow + UInt32(byte)) % 65521
            sumHigh = (sumHigh + sumLow) % 65521
        }
        return (sumHigh << 16) | sumLow
    }

    /// Appends a Samsung-style SEFT trailer with `MCC_Data` (`404`) after JPEG `EOI`.
    static func jpegWithMakerTrailer(_ jpeg: Data) -> Data {
        var out = jpeg
        out.append(Data([0x00, 0x00, 0x01, 0x0A]))
        out.append(Data("MCC_Data404SEFT".utf8))
        return out
    }

    /// Appends an MP4 trailer carrying a QuickTime `©xyz` atom after JPEG `EOI`.
    static func motionPhoto(_ jpeg: Data) -> Data {
        var ftypPayload = [UInt8]("mp42".utf8)
        ftypPayload += [0x00, 0x00, 0x00, 0x00]
        ftypPayload += [UInt8]("mp42isom".utf8)
        let ftyp = box("ftyp", payload: ftypPayload)

        var xyzPayload: [UInt8] = [0x00, 0x12, 0x15, 0xC7]
        xyzPayload += [UInt8]("+31.6200+074.8765/".utf8)
        let xyz = rawBox([0xA9, 0x78, 0x79, 0x7A], payload: xyzPayload)
        let udta = box("udta", payload: xyz)
        let moov = box("moov", payload: udta)
        let mdat = box("mdat", payload: [0x00, 0x00, 0x00, 0x00])

        var out = jpeg
        out.append(contentsOf: ftyp + moov + mdat)
        return out
    }

    static func box(_ kind: String, payload: [UInt8]) -> [UInt8] {
        rawBox([UInt8](kind.utf8), payload: payload)
    }

    static func rawBox(_ kind: [UInt8], payload: [UInt8]) -> [UInt8] {
        word(UInt32(8 + payload.count)) + kind + payload
    }

    /// Writes a short 64x64 H.264 `.mov` file, optionally carrying QuickTime
    /// ISO-6709 location metadata for Amritsar.
    static func film(in folder: URL, named name: String, withPlace: Bool) async throws -> URL {
        let url = folder.appendingPathComponent(name)
        try? FileManager.default.removeItem(at: url)
        let writer = try AVAssetWriter(outputURL: url, fileType: .mov)
        if withPlace {
            let location = AVMutableMetadataItem()
            location.identifier = .quickTimeMetadataLocationISO6709
            location.dataType = kCMMetadataBaseDataType_UTF8 as String
            location.value = "+31.6200+074.8765+234.0/" as NSString
            writer.metadata = [location]
        }
        let settings: [String: Any] = [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: 64,
            AVVideoHeightKey: 64,
        ]
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
        let attributes: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: Int(kCVPixelFormatType_32BGRA),
            kCVPixelBufferWidthKey as String: 64,
            kCVPixelBufferHeightKey as String: 64,
        ]
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(
            assetWriterInput: input, sourcePixelBufferAttributes: attributes
        )
        #expect(writer.canAdd(input))
        writer.add(input)
        #expect(writer.startWriting())
        writer.startSession(atSourceTime: .zero)

        for frame in 0..<2 {
            while !input.isReadyForMoreMediaData {
                try await Task.sleep(for: .milliseconds(5))
            }
            let pixelBuffer = try makeFrameBuffer(attributes: attributes)
            let time = CMTime(value: CMTimeValue(frame), timescale: 10)
            #expect(adaptor.append(pixelBuffer, withPresentationTime: time))
        }
        input.markAsFinished()
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            writer.finishWriting { continuation.resume() }
        }
        #expect(writer.status == .completed)
        return url
    }

    private static func makeFrameBuffer(attributes: [String: Any]) throws -> CVPixelBuffer {
        var buffer: CVPixelBuffer?
        let status = CVPixelBufferCreate(
            kCFAllocatorDefault, 64, 64, kCVPixelFormatType_32BGRA, attributes as CFDictionary, &buffer
        )
        #expect(status == kCVReturnSuccess)
        let pixelBuffer = try #require(buffer)
        CVPixelBufferLockBaseAddress(pixelBuffer, [])
        if let base = CVPixelBufferGetBaseAddress(pixelBuffer) {
            memset(base, 0x90, CVPixelBufferGetDataSize(pixelBuffer))
        }
        CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
        return pixelBuffer
    }
}
