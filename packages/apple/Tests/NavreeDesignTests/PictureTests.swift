import Foundation
import Testing

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
