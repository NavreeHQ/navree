import Foundation
import NavreeCoreBridge
import NavreeProtocol
import Testing

@testable import NavreeRuntime
@testable import NavreeShell

/// A window whose core stopped: what was kept of it stays, the start that
/// follows is a plain one, and what was kept is set aside for that start only.
@MainActor
struct StoppedCoreTests {
    /// One window over a session of the test's own, answered from the store's recorded files.
    struct Window {
        let session: Session
        let keeper: SessionKeeper
        let folder: URL

        @MainActor
        init() {
            folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-stopped-\(UUID().uuidString)")
            keeper = SessionKeeper(
                cache: KeptFiles(folder: folder.appendingPathComponent("cache")),
                snapshots: KeptFiles(folder: folder.appendingPathComponent("snapshot")),
                files: FileStore(folder: folder.appendingPathComponent("files"))
            )
            let configuration = ShellConfiguration(
                origin: "https://shop.example", appID: "com.navree.demo", appVersion: "1.0", signedWith: [],
                transport: {
                    let transport = FakeTransport()
                    transport.answers = Shell.store
                    return transport
                }
            )
            session = Session(configuration: configuration, keeper: keeper)
        }
    }

    @Test func whatWasKeptStaysWhenTheCoreStopsAndTheStartThatFollowsIsPlain() throws {
        let window = Window()
        defer { try? FileManager.default.removeItem(at: window.folder) }
        let kept = Data(#"{"version":1,"typed":"Ship to Harleen Sandhu, Model Town, Ludhiana"}"#.utf8)
        window.keeper.snapshots.writeData("snapshot-first", kept)
        // The system brings the window back with what was kept of it.
        window.session.begin(restoring: true, scene: "first")
        let client = try #require(window.session.client)
        #expect(client.began)
        window.session.window(.expanded)
        #expect(window.keeper.snapshots.readData("snapshot-first") == kept)

        // The core fails inside and stops.
        client.noteStop(CoreAnswer.stoppedWords)
        #expect(client.stopped)
        #expect(window.keeper.snapshots.holds("snapshot-first-stopped"))
        // The app goes to the background: what was kept is not written over with nothing.
        window.session.keep()
        #expect(window.keeper.snapshots.readData("snapshot-first") == kept)

        // The person starts the app again.
        window.session.restart()
        let again = try #require(window.session.client)
        #expect(again !== client)
        #expect(!again.stopped)
        #expect(again.nav.stage == .ready, "it started plainly and its shell arrived")
        #expect(again.nav.width == .expanded, "the new core was told the window's width")
        // What was kept is not given back, and is set aside for this start only.
        #expect(window.keeper.snapshots.readData("snapshot-first") == nil)
        #expect(window.keeper.snapshots.readData("snapshot-first-aside") == kept)
        #expect(!window.keeper.snapshots.holds("snapshot-first-stopped"))
        // A second restart of a core that runs does nothing.
        window.session.restart()
        #expect(window.session.client === again)
    }

    @Test func aLaunchAfterAStopIsPlainOnceAndTheOneAfterItIsAsEver() {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("navree-start-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        let store = KeptFiles(folder: folder)
        let kept = Data(#"{"version":1}"#.utf8)
        store.writeData("snapshot-first", kept)
        store.write(StartRules.stoppedKey("snapshot-first"), "stopped")
        // The person quit after the stop, and the system brings the window back.
        #expect(StartRules.snapshot(restoring: true, kept: "snapshot-first", in: store) == nil)
        #expect(store.readData(StartRules.asideKey("snapshot-first")) == kept)
        #expect(store.readData("snapshot-first") == nil)
        // That start worked, and the app went away again with something new kept.
        let later = Data(#"{"version":1,"later":true}"#.utf8)
        store.writeData("snapshot-first", later)
        #expect(StartRules.snapshot(restoring: true, kept: "snapshot-first", in: store) == later)
        #expect(store.readData(StartRules.asideKey("snapshot-first")) == nil)
        // A window that starts plainly lets go of what was kept of it.
        #expect(StartRules.snapshot(restoring: false, kept: "snapshot-first", in: store) == nil)
        #expect(store.readData("snapshot-first") == nil)
    }
}
