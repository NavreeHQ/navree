import CoreGraphics
import Foundation
import ImageIO
import NavreeRuntime
import Testing
import UniformTypeIdentifiers

@testable import NavreeShell

/// The place a photo was taken is taken out of it before a form sends it,
/// unless the field asks for the place.
@MainActor
struct PhotoPlaceTests {
    /// A folder of the test's own.
    static func folder() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("navree-photos-\(UUID().uuidString)")
    }

    /// A small photo written as a JPEG, with the place it was taken (the
    /// Golden Temple, Amritsar) when `placed`.
    static func photo(_ name: String, placed: Bool, in folder: URL) throws -> URL {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(name)
        let context = try #require(CGContext(
            data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
        ))
        context.setFillColor(red: 0.95, green: 0.6, blue: 0.1, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: 8, height: 8))
        let image = try #require(context.makeImage())
        let destination = try #require(CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil))
        var properties: [CFString: Any] = [:]
        if placed {
            let place: [CFString: Any] = [
                kCGImagePropertyGPSLatitude: 31.62, kCGImagePropertyGPSLatitudeRef: "N",
                kCGImagePropertyGPSLongitude: 74.8765, kCGImagePropertyGPSLongitudeRef: "E",
            ]
            properties[kCGImagePropertyGPSDictionary] = place
        }
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        let written = CGImageDestinationFinalize(destination)
        #expect(written)
        return url
    }

    /// Whether the picture in a file names a place.
    static func holdsPlace(_ url: URL) throws -> Bool {
        PhotoPlace.holdsPlace(try #require(CGImageSourceCreateWithURL(url as CFURL, nil)))
    }

    @Test func thePlaceIsTakenOutOfAPhotoBeforeItIsKeptForAForm() throws {
        let folder = Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = try Self.photo("receipt.jpg", placed: true, in: folder)
        let taken = try Self.holdsPlace(photo)
        #expect(taken, "the photo the test made names where it was taken")

        let store = FileStore(folder: folder.appendingPathComponent("kept"))
        let kept = try #require(store.keep(photo))
        #expect(kept.name == "receipt.jpg")
        #expect(kept.type == "image/jpeg")
        let bytes = try #require(store.data(kept.token))
        #expect(kept.size == bytes.count)
        // What a form sends is still the picture, and says nothing of where it was taken.
        let sent = folder.appendingPathComponent("sent.jpg")
        try bytes.write(to: sent)
        let still = try Self.holdsPlace(sent)
        #expect(!still)
        let picture = try #require(CGImageSourceCreateWithURL(sent as CFURL, nil))
        #expect(CGImageSourceGetCount(picture) == 1)
        let size = CGImageSourceCopyPropertiesAtIndex(picture, 0, nil) as? [String: Any]
        #expect(size?[kCGImagePropertyPixelWidth as String] as? Int == 8)
        // The photo the person picked is left as it was.
        let original = try Self.holdsPlace(photo)
        #expect(original)
    }

    @Test func aFieldThatAsksForThePlaceIsSentThePhotoAsItIs() throws {
        let folder = Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let photo = try Self.photo("site.jpg", placed: true, in: folder)
        let store = FileStore(folder: folder.appendingPathComponent("kept"))
        let kept = try #require(store.keep(photo, keepLocation: true))
        #expect(store.data(kept.token) == (try Data(contentsOf: photo)))
    }

    @Test func aPhotoThatNamesNoPlaceAndAFileThatIsNoPictureAreKeptByteForByte() throws {
        let folder = Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let store = FileStore(folder: folder.appendingPathComponent("kept"))
        let photo = try Self.photo("label.jpg", placed: false, in: folder)
        let kept = try #require(store.keep(photo))
        #expect(store.data(kept.token) == (try Data(contentsOf: photo)))

        let notes = folder.appendingPathComponent("notes.txt")
        try Data("Wrap the dupattas in tissue".utf8).write(to: notes)
        #expect(!PhotoPlace.isPicture(notes))
        let other = try #require(store.keep(notes))
        #expect(store.data(other.token) == Data("Wrap the dupattas in tissue".utf8))
    }

    @Test func aPictureIsKnownByWhatIsInItWhateverItsName() throws {
        let folder = Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        // A photo sent under a name that says nothing of what it is.
        let photo = try Self.photo("upload.bin", placed: true, in: folder)
        #expect(PhotoPlace.isPicture(photo))
        let store = FileStore(folder: folder.appendingPathComponent("kept"))
        let kept = try #require(store.keep(photo))
        let sent = folder.appendingPathComponent("sent.bin")
        try #require(store.data(kept.token)).write(to: sent)
        let still = try Self.holdsPlace(sent)
        #expect(!still)
    }

    @Test func aPictureWhosePlaceCannotBeLookedForIsNotKeptAtAll() throws {
        let folder = Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        // A picture by its name, and nothing that reads as one: whether it
        // names a place cannot be told, so it is not sent as it is.
        let broken = folder.appendingPathComponent("holiday.jpg")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("not a picture".utf8).write(to: broken)
        let kept = folder.appendingPathComponent("kept")
        let store = FileStore(folder: kept)
        #expect(store.keep(broken) == nil)
        // Nothing of it is left among the kept files.
        let left = (try? FileManager.default.contentsOfDirectory(atPath: kept.path)) ?? []
        #expect(left.isEmpty)
        // A field that asks for the place takes the file as it is.
        #expect(store.keep(broken, keepLocation: true) != nil)
    }
}
