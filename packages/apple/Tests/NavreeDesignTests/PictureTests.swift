import CoreGraphics
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers

@testable import NavreeDesign

@Test func aLoadedPictureIsShownOnlyWhereItsOwnAddressIsAskedFor() {
    let harleen = URL(string: "https://shop.example/people/harleen-sandhu.jpg")
    let gurpreet = URL(string: "https://shop.example/people/gurpreet-gill.jpg")
    #expect(PictureRules.shows(loaded: harleen, asked: harleen))
    // The row now stands for someone else: the face that was there is not shown beside the new name.
    #expect(!PictureRules.shows(loaded: harleen, asked: gurpreet))
    // Nor where no picture is asked for any more, or none was loaded.
    #expect(!PictureRules.shows(loaded: harleen, asked: nil))
    #expect(!PictureRules.shows(loaded: nil, asked: gurpreet))
    #expect(!PictureRules.shows(loaded: nil, asked: nil))
}

/// A picture of one colour, `width` by `height` pixels, as the bytes of a PNG.
private func picture(width: Int, height: Int) throws -> Data {
    let context = try #require(CGContext(
        data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
    ))
    context.setFillColor(red: 0.1, green: 0.5, blue: 0.4, alpha: 1)
    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = try #require(context.makeImage())
    let data = NSMutableData()
    let destination = try #require(
        CGImageDestinationCreateWithData(data as CFMutableData, UTType.png.identifier as CFString, 1, nil)
    )
    CGImageDestinationAddImage(destination, image, nil)
    let written = CGImageDestinationFinalize(destination)
    #expect(written)
    return data as Data
}

@Test func aPictureLargerThanAnyScreenIsReadSmallerAndASmallOneAsItIs() throws {
    // A banner far wider than a screen shows: read no wider than the most a screen needs, its shape kept.
    let wide = try #require(PictureRules.decoded(try picture(width: PictureRules.mostPixels * 2, height: 64)))
    #expect(wide.width == PictureRules.mostPixels)
    #expect(wide.height == 32)
    // A face for a row is read as it came.
    let small = try #require(PictureRules.decoded(try picture(width: 96, height: 64)))
    #expect(small.width == 96)
    #expect(small.height == 64)
    // What is no picture is none.
    #expect(PictureRules.decoded(Data("not a picture".utf8)) == nil)
}
