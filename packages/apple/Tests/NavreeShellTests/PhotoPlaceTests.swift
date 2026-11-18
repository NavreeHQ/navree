import CoreGraphics
import Foundation
import ImageIO
import NavreeRuntime
import Testing
import UniformTypeIdentifiers

@testable import NavreeShell

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
                == "Kapurthala.dng holds details that can say where it was taken, and they cannot be taken out of this kind of file, so it was not added."
        )
    }
}
