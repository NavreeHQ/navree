import Foundation
import Testing

@testable import NavreeRuntime
@testable import NavreeShell

/// What is stored of a window when the app goes away.
@MainActor
struct KeepTests {
    @Test func whatTheCoreDoesNotKnowThePersonOfIsNotStored() {
        let known = #"{"version":1,"origin":"https://shop.example","person":"9f3c","selected":"/orders"}"#
        let nobody = #"{"version":1,"origin":"https://shop.example"}"#
        let unknown = #"{"version":1,"origin":"https://shop.example","person_unknown":true}"#
        #expect(StartRules.stores(Data(known.utf8)))
        #expect(StartRules.stores(Data(nobody.utf8)))
        #expect(!StartRules.stores(Data(unknown.utf8)))
        // Only the core's own word says so.
        #expect(StartRules.stores(Data(#"{"version":1,"person_unknown":false}"#.utf8)))
        #expect(StartRules.stores(Data(#"{"version":1,"person_unknown":"true"}"#.utf8)))
    }

    @Test func aPersonsPlaceIsNotWrittenOverWhileTheCoreDoesNotKnowWhoseItIs() throws {
        let window = StoppedCoreTests.Window()
        defer { try? FileManager.default.removeItem(at: window.folder) }
        window.session.begin(restoring: false, scene: "first")
        let client = try #require(window.session.client)
        #expect(client.nav.stage == .ready)
        // The store's answers here name Ravneet Kaur without an id, and the
        // core keeps nothing for a person it cannot tell from another.
        let given = try #require(client.snapshot())
        #expect(!StartRules.stores(given))

        let place = Data(#"{"version":1,"typed":"Ship to Harleen Sandhu, Model Town, Ludhiana"}"#.utf8)
        window.keeper.snapshots.writeData("snapshot-first", place)
        window.session.keep()
        #expect(window.keeper.snapshots.readData("snapshot-first") == place)
    }
}
