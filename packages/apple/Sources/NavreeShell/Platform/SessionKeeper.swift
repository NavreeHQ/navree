import Foundation
import NavreeProtocol
import NavreeRuntime
import WebKit
import os

/// The app's one session at its origin, for every window: one cookie jar and
/// its one Keychain item, one web view store, one place for the pages the
/// cores keep and one for what is kept of each window, and the session's
/// number, which goes up each time the session ends.
///
/// Each window runs a core of its own, and every one of them is signed in as
/// the same person or as nobody. So the end of a session is the app's, not a
/// window's: whichever window's core says the person signed out, the session
/// ends for all of them, at once and in this order.
///
/// 1. The number goes up. Whatever was begun for the person who is leaving
///    carries the older number and is refused from here on: an answer's
///    cookies, a page to keep, a snapshot, what a web page set, a picture.
/// 2. A mark is written to disk: a sign-out has begun.
/// 3. The requests and streams under way are dropped.
/// 4. What could bring the session back goes: the cookies, from memory and
///    from the Keychain; then the web view's store, which is never used
///    again (the next session's is a new one).
/// 5. Everything else kept of the person goes, for every window: the pages,
///    what was kept of each window, the files picked for forms, the pictures,
///    the notifications and the count on the app's icon.
/// 6. Every other window's core is told, and starts again as nobody's.
/// 7. The mark is taken away, only when everything before it finished and
///    the core has said the session is forgotten.
///
/// A core says a sign-out in one effect or in two. With no push token to
/// take back it says `forget_session` at once (``end(by:)``). With one, it
/// first asks the server to forget the token, which may take seconds, and
/// says `session_ending` in the same update, after that request
/// (``ending(by:)``); `forget_session` follows when the request is answered.
/// All of the above is done at `session_ending`, so an app ended while the
/// request is out has already taken everything away, and the mark, which
/// stays until `forget_session`, makes the next start do it once more. One
/// thing is spared at `session_ending`: the requests of the window whose core
/// is signing out, since the token's removal is one of them. It was made
/// before the effect, so it left with the cookies it needed; the cookies its
/// answer sets are refused like any other answer's of the ended session, and
/// the core itself has cancelled every other request it had out. The
/// `forget_session` that follows finds the work done and does none of it again.
///
/// A start that finds the mark does the same again before the Keychain
/// is read or anything kept is opened (``open(origin:loopbackHTTP:slot:cache:snapshots:files:record:removes:)``),
/// and takes nothing from the Keychain in that run. The stores of sessions
/// that ended are removed when the system lets them be (no web view may still
/// use one), and what could not be removed is tried again at each start.
@MainActor
final class SessionKeeper {
    /// The session's number.
    private(set) var number = 0
    /// Where every window's core keeps its pages: one store, since every core
    /// is the same person's. Each core lists what it keeps for itself, so a
    /// second window can cost the first some of the pages it would have been
    /// shown at once at the next launch, and nothing else.
    let cache: KeptFiles
    /// Where what is kept of each window for a launch the system may end is
    /// written, a file for each window.
    let snapshots: KeptFiles
    /// The files the person picked for forms.
    let files: FileStore
    /// The token given up when a session ended is asked for again once
    /// somebody is signed in, in whichever window that is.
    var pushAwaitsSignIn = false
    /// A sign-out the run before began was finished as this one started.
    private(set) var finishedAtStart = false

    private let origin: String?
    private let loopbackHTTP: Bool
    private let slot: (any SecretSlot)?
    private let record: SessionRecord?
    private let removes: @MainActor (UUID) async -> Bool
    /// Whether the jar, when it is first made, takes what the Keychain holds.
    private var readsSlot = true
    private var made: CookieJar?
    private var windows: [Weak<SystemEffects>] = []
    /// The web view's store of the session, and the number it was made under.
    private var web: (number: Int, store: WKWebsiteDataStore)?
    /// The list of stores could not be written: no store kept between
    /// launches is used until a start has put it right.
    private var storesInDoubt = false
    /// A main window has started in this run.
    private var begun = false
    /// A sign-out a core has said is ending and not yet said is forgotten:
    /// the window whose core it is, and whether everything on the device
    /// was taken away (so the mark may go when the core says the rest).
    private var underWay: (window: Weak<SystemEffects>, finished: Bool)?

    nonisolated static let log = Logger(subsystem: "com.navree.native", category: "session")

    /// How long what is kept of a window stays when nothing writes it again:
    /// a window the person closed never asks for it, and nothing says so.
    static let unusedLimit: TimeInterval = 30 * 24 * 60 * 60

    /// A session over `jar` (or, with `origin`, a jar made when a transport
    /// first asks for one), keeping its pages in `cache` and its windows in
    /// `snapshots`. With a `record`, its end survives the app ending in the
    /// middle of it and its web view store is kept between launches.
    init(
        jar: CookieJar? = nil, origin: String? = nil, loopbackHTTP: Bool = false, slot: (any SecretSlot)? = nil,
        cache: KeptFiles, snapshots: KeptFiles, files: FileStore = .shared, record: SessionRecord? = nil,
        removes: @escaping @MainActor (UUID) async -> Bool = SessionKeeper.removeSystemStore
    ) {
        made = jar
        self.origin = origin
        self.loopbackHTTP = loopbackHTTP
        self.slot = slot
        self.cache = cache
        self.snapshots = snapshots
        self.files = files
        self.record = record
        self.removes = removes
    }

    // MARK: - The start

    /// Opens the session kept on this device, before anything of it is read.
    /// A sign-out that was cut short is finished first: the Keychain item,
    /// the web view's store, the pages, what was kept of each window and the
    /// picked files go, the notifications too, and the jar of this run takes
    /// nothing from the Keychain whether or not its item could be deleted.
    /// The mark stays until all of it is gone. Then the stores of sessions
    /// that ended are removed, and a folder standing among the kept files
    /// (none of them is one) is deleted.
    static func open(
        origin: String, loopbackHTTP: Bool = false, slot: (any SecretSlot)? = nil, cache: KeptFiles,
        snapshots: KeptFiles, files: FileStore = .shared, record: SessionRecord? = nil,
        removes: @escaping @MainActor (UUID) async -> Bool = SessionKeeper.removeSystemStore
    ) -> SessionKeeper {
        let keeper = SessionKeeper(
            origin: origin, loopbackHTTP: loopbackHTTP, slot: slot, cache: cache, snapshots: snapshots, files: files,
            record: record, removes: removes
        )
        if let record, record.owesWipe {
            keeper.finishedAtStart = true
            keeper.readsSlot = false
            var finished = slot?.delete() ?? true
            let retired = record.retireStore()
            keeper.storesInDoubt = !retired
            finished = retired && finished
            finished = keeper.clearKept() && finished
            SystemAppNotifications().signedOut()
            if finished { record.unmarkWipe() }
            log.notice("a sign-out the last run began was finished at this start: \(finished ? "all of it" : "not all of it yet", privacy: .public)")
        }
        cache.clearFolders()
        snapshots.clearFolders()
        keeper.removeRetired()
        return keeper
    }

    /// Every session of the process, by its origin and how it is kept.
    private static var sessions: [String: SessionKeeper] = [:]

    /// The one session of the app built with `configuration`.
    static func shared(for configuration: ShellConfiguration) -> SessionKeeper {
        shared(origin: configuration.origin, loopbackHTTP: configuration.loopbackHTTP, keepsSession: configuration.keepsSession)
    }

    /// The one session of the app at `origin`, opened the first time it is asked for.
    static func shared(origin: String, loopbackHTTP: Bool, keepsSession: Bool) -> SessionKeeper {
        let name = "\(origin) \(loopbackHTTP) \(keepsSession)"
        if let keeper = sessions[name] { return keeper }
        let keeper = open(
            origin: origin, loopbackHTTP: loopbackHTTP,
            slot: keepsSession ? KeychainSlot(service: CookieJar.service, account: origin) : nil,
            cache: KeptFiles(Session.cacheFolder(origin), in: .cachesDirectory), snapshots: KeptFiles("navree-snapshot"),
            record: SessionRecord(origin: origin, keepsWebStore: keepsSession)
        )
        sessions[name] = keeper
        return keeper
    }

    // MARK: - The cookies

    /// The session's jar, when a transport has asked for it or the session was made over one.
    var jar: CookieJar? { made }

    /// The jar the network's requests carry, the same for every window: made
    /// the first time it is asked for, from the Keychain item unless a
    /// sign-out was finished at this start.
    func networkJar() -> CookieJar {
        if let made { return made }
        let jar = CookieJar(origin: origin ?? "", loopbackHTTP: loopbackHTTP, slot: slot, reads: readsSlot)
        made = jar
        return jar
    }

    // MARK: - The windows

    /// A window's effects take part in the session.
    func join(_ window: SystemEffects) {
        settle()
        windows.removeAll { $0.object == nil }
        windows.append(Weak(window))
    }

    private func live() -> [SystemEffects] {
        settle()
        windows.removeAll { $0.object == nil }
        return windows.compactMap(\.object)
    }

    /// A sign-out whose window went away before its core said the session
    /// was forgotten has nothing more to wait for: its mark goes, if
    /// everything on the device was taken away.
    private func settle() {
        guard let said = underWay, said.window.object == nil else { return }
        underWay = nil
        if said.finished { record?.unmarkWipe() }
    }

    /// A window's core is about to start. The first main window of a run
    /// decides what stays of the windows before it: when the system brings
    /// it back, what was kept of each window stays for the window to ask for,
    /// unless nothing has written it for ``unusedLimit``; when it starts
    /// plainly, no window of the run before is coming back with its place,
    /// and all of it goes. `restoring` is `nil` for a window that is not one
    /// of the app's main ones, which decides nothing.
    func windowBegins(restoring: Bool?, now: Date = Date()) {
        guard let restoring, !begun else { return }
        begun = true
        if restoring {
            snapshots.clear(writtenBefore: now.addingTimeInterval(-Self.unusedLimit))
        } else {
            snapshots.clear()
        }
    }

    // MARK: - The end

    /// A window's core says the session is ending (`session_ending`): the
    /// person signed out, and the core is still asking the server to forget
    /// this install's push token. Everything a sign-out takes away on the
    /// device goes now, for every window, as the type's own words set out;
    /// only the requests of `window` itself go on, the token's removal among
    /// them, and the mark stays until its core says the session is forgotten.
    func ending(by window: SystemEffects) {
        let finished = takeAway(by: window, sparing: true)
        underWay = (Weak(window), finished)
    }

    /// A window's core says the session is forgotten (`forget_session`).
    /// When the same core said before that it was ending, everything is
    /// done and nothing is done twice: the mark goes, if all of it finished.
    /// Otherwise the session ends here, for every window. `ender` is the
    /// window whose core said so: it goes on in the session that follows,
    /// and every other window's core is told.
    func end(by ender: SystemEffects? = nil) {
        if let said = underWay, let ender, said.window.object === ender {
            underWay = nil
            if said.finished { record?.unmarkWipe() }
            return
        }
        if takeAway(by: ender, sparing: false) { record?.unmarkWipe() }
    }

    /// Takes away everything of the session on this device, for every
    /// window, and moves the session's number on. With `sparing`, the
    /// requests of `ender` itself are left to be answered. `true` when the
    /// mark was written and everything is gone: the mark may then go.
    private func takeAway(by ender: SystemEffects?, sparing: Bool) -> Bool {
        underWay = nil
        number += 1
        let marked = record?.markWipe() ?? true
        let windows = live()
        for window in windows where !(sparing && window === ender) { window.stopWork() }
        // What could bring the session back: the cookies, then the web view's store.
        var finished = true
        var jars: [CookieJar] = []
        for jar in [made] + windows.map({ $0.transport.jar }) {
            if let jar, !jars.contains(where: { $0 === jar }) { jars.append(jar) }
        }
        for jar in jars { finished = jar.forget() && finished }
        if made == nil, let slot { finished = slot.delete() && finished }
        let old = web?.store
        web = nil
        let retired = record?.retireStore() ?? true
        storesInDoubt = storesInDoubt || !retired
        finished = retired && finished
        // Then everything else kept of the person.
        finished = clearKept() && finished
        for window in windows { window.pictures.forget() }
        if let teller = ender ?? windows.first {
            teller.devices.notifications.signedOut()
        } else {
            SystemAppNotifications().signedOut()
        }
        for window in windows where window !== ender { window.sessionEnded() }
        ender?.caughtUp()
        removeRetired(emptying: old)
        return marked && finished
    }

    /// Who a window's core says is signed in, as far as a client can tell
    /// two people apart: by name and address.
    struct Person: Equatable {
        var name: String
        var email: String?
    }

    /// A window's core now names another person than it did, and nobody
    /// signed out between: a session ran out, and somebody else signed in.
    /// What a core keeps it empties itself. What is the person's and not a
    /// core's goes here, for every window: the pictures loaded, the files
    /// picked for forms, the notifications and the count on the app's icon.
    func personChanged(by window: SystemEffects) {
        for window in live() { window.pictures.forget() }
        files.clear()
        window.devices.notifications.signedOut()
    }

    /// Deletes the pages, what was kept of each window, and the picked files.
    private func clearKept() -> Bool {
        let pages = cache.clear()
        let places = snapshots.clear()
        let picked = files.clear()
        return pages && places && picked
    }

    // MARK: - The web view's store

    /// The web view's store of the session as it is now. Each session has a
    /// store of its own (`WKWebsiteDataStore(forIdentifier:)`), named in the
    /// record before it is first made, so a page of the next person finds
    /// nothing a page of the last one left, and the same session finds its
    /// own again at the next launch. A session that is not kept between
    /// launches, or whose store could not be written down, gets a store that
    /// lives in memory only.
    func webStore() -> WKWebsiteDataStore {
        if let web, web.number == number { return web.store }
        let store: WKWebsiteDataStore
        if let id = storeIdentifier() {
            store = WKWebsiteDataStore(forIdentifier: id)
        } else {
            store = .nonPersistent()
        }
        web = (number, store)
        return store
    }

    /// The identifier of the session's store, written down before it is used.
    func storeIdentifier() -> UUID? {
        guard let record, record.keepsWebStore, !storesInDoubt, !record.owesWipe else { return nil }
        var list = record.stores()
        if let current = list.current { return current }
        let new = UUID()
        list.current = new
        return record.write(list) ? new : nil
    }

    /// The removal of ended sessions' stores under way, for a test to wait for.
    private(set) var removal: Task<Void, Never>?

    /// Removes the stores of sessions that ended. `old`, the store of the
    /// session that just ended, is emptied first, which the system does even
    /// while a web view still shows a page from it; removing a store needs
    /// every web view that used it gone, so it is tried a few times as the
    /// windows let go of theirs. A store leaves the list only when it is
    /// gone; what is left is tried again at the next start.
    func removeRetired(emptying old: WKWebsiteDataStore? = nil) {
        let emptied = Task { @MainActor in
            if let old {
                await old.removeData(ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(), modifiedSince: .distantPast)
            }
        }
        guard let record else { return }
        let removes = removes
        let before = removal
        removal = Task { @MainActor in
            await before?.value
            await emptied.value
            for wait in [0, 1, 2, 4, 8] {
                if wait > 0 { try? await Task.sleep(for: .seconds(wait)) }
                let waiting = record.stores().retired
                if waiting.isEmpty { return }
                var gone: [UUID] = []
                for id in waiting {
                    if await removes(id) { gone.append(id) }
                }
                // Read again: a session may have ended meanwhile and added its own.
                var list = record.stores()
                list.retired.removeAll { gone.contains($0) }
                _ = record.write(list)
                if list.retired.isEmpty { return }
            }
            SessionKeeper.log.notice("a web view store of a session that ended could not be removed yet: it is tried again at the next start")
        }
    }

    /// Removes one of the system's web view stores; `true` when it is gone,
    /// or was never made.
    static func removeSystemStore(_ id: UUID) async -> Bool {
        guard await WKWebsiteDataStore.allDataStoreIdentifiers.contains(id) else { return true }
        do {
            try await WKWebsiteDataStore.remove(forIdentifier: id)
            return true
        } catch {
            return false
        }
    }
}

/// What the session writes down about itself, in a folder of the app's own
/// that no backup takes: that a sign-out has begun and not finished, and
/// which web view store is the session's and which belong to sessions that
/// ended. Nothing in it is a secret, and it can be read whenever the Keychain
/// item can (after the device's first unlock).
struct SessionRecord: Sendable {
    let folder: URL
    /// Whether the session's web view store is kept between launches.
    let keepsWebStore: Bool

    /// The record of the session at `origin`, in the app's Application Support.
    init(origin: String, keepsWebStore: Bool = true) {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let name = "navree-session-" + String(Session.cacheFolder(origin).dropFirst("navree-cache-".count))
        self.init(folder: base.appendingPathComponent(name, isDirectory: true), keepsWebStore: keepsWebStore)
    }

    init(folder: URL, keepsWebStore: Bool = true) {
        self.folder = folder
        self.keepsWebStore = keepsWebStore
    }

    private var mark: URL { folder.appendingPathComponent("signing-out") }
    private var list: URL { folder.appendingPathComponent("web-stores.json") }

    /// Whether a sign-out began and did not finish.
    var owesWipe: Bool {
        FileManager.default.fileExists(atPath: mark.path)
    }

    /// Writes down that a sign-out has begun, and has the system put it on
    /// the disk before anything is deleted. `false` when it could not be.
    func markWipe() -> Bool {
        do {
            try prepare()
            try Data("1".utf8).write(to: mark, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            let handle = try FileHandle(forWritingTo: mark)
            try handle.synchronize()
            try handle.close()
            return true
        } catch {
            return false
        }
    }

    /// Takes the mark away: the sign-out finished.
    @discardableResult
    func unmarkWipe() -> Bool {
        try? FileManager.default.removeItem(at: mark)
        return !owesWipe
    }

    /// The web view stores written down; none when nothing reads.
    func stores() -> WebStoreList {
        guard let data = try? Data(contentsOf: list), let read = try? JSONDecoder().decode(WebStoreList.self, from: data)
        else { return WebStoreList() }
        return read
    }

    func write(_ stores: WebStoreList) -> Bool {
        do {
            try prepare()
            try JSONEncoder().encode(stores).write(
                to: list, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication]
            )
            return true
        } catch {
            return false
        }
    }

    /// The session's store becomes one of a session that ended: it is never
    /// given to a web view again. `false` when that could not be written down.
    func retireStore() -> Bool {
        var list = stores()
        guard let current = list.current else { return true }
        list.current = nil
        list.retired.append(current)
        return write(list)
    }

    private func prepare() throws {
        guard !FileManager.default.fileExists(atPath: folder.path) else { return }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        var folder = folder
        try folder.setResourceValues(values)
    }
}

/// The web view stores of a session and of the sessions before it.
struct WebStoreList: Codable, Equatable, Sendable {
    /// The store of the session as it is now; none until a web page is first shown in it.
    var current: UUID?
    /// The stores of sessions that ended, until each is removed.
    var retired: [UUID] = []
}
