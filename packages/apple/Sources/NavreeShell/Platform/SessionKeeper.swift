import CryptoKit
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
/// The mark names the session it ends, by a count the record keeps across
/// launches. While a mark stands, the session after it writes nothing to the
/// device (no cookie in the Keychain, no page, nothing of a window, no web
/// view store) before the count has moved past the mark; and the count moves
/// only once everything of the ended session is off the device. So a start
/// that finds a mark naming the session the device still holds finishes that
/// sign-out and can take nothing of a later person with it; and a mark the
/// count has moved past is an earlier session's, and ends nothing.
///
/// Who is signed in is the session's too, heard from every window's core and
/// written down: the key the core gives for the person, and a mark made from
/// the name and address it names them by, from which neither can be read.
/// When a core names somebody else with no sign-out between (a session ran
/// out, and another person signed in), what was the last person's goes for
/// every window, and every other window's core starts again
/// (``hears(_:from:)``), also when the two are a launch apart.
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
    /// Who is signed in, as the windows' cores last named them: nobody
    /// since the last sign-out.
    private(set) var person: Person?
    /// How often a core's word that the session is forgotten was passed
    /// over, because the session it spoke of had been ended already.
    private(set) var passedOver = 0

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
    /// The window whose core has said the session is ending and not yet
    /// that it is forgotten: the one the mark waits for.
    private var underWay: Weak<SystemEffects>?
    /// Everything is being taken away now: a sign-out said meanwhile, from
    /// inside it, has nothing left to take.
    private var taking = false

    /// Where a window's core stands in a sign-out it said in two words.
    enum Tail: Sendable {
        /// It is in none.
        case none
        /// It said the session is ending: everything was taken away then,
        /// and its word that the session is forgotten is still to come.
        case awaited
        /// It was then told the session ended, by another window's sign-out
        /// or a change of person. A core either says the session is
        /// forgotten in answer to that, before it asks for anything new, or
        /// never says it for that sign-out.
        case told
    }
    /// The mark of a sign-out that still stands on the device.
    private var standing: Standing?
    /// A sign-out finished at this start has not yet given the push token up.
    private var owesPushRemoval = false

    /// What a sign-out could not take off the device.
    struct Left: OptionSet, Sendable {
        let rawValue: Int
        static let keychain = Left(rawValue: 1 << 0)
        static let store = Left(rawValue: 1 << 1)
        static let pages = Left(rawValue: 1 << 2)
        static let places = Left(rawValue: 1 << 3)
        static let files = Left(rawValue: 1 << 4)
        static let person = Left(rawValue: 1 << 5)
    }

    /// A sign-out whose mark stands on the device.
    private struct Standing {
        /// The session it ends, by the record's count.
        var session: Int
        /// What of that session could not be taken off the device yet.
        var left: Left
        /// A later session has written to the device since: the record's
        /// count has moved past this mark, which ends nothing any more.
        var passed = false
    }

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
        jar?.mayKeep = { [weak self] in self?.passMark(savingCookies: false) ?? true }
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
        if let record, let marked = record.marked() {
            if marked >= record.serial() {
                // No session has written to the device since that sign-out
                // began: it is finished here, and takes nobody else's with it.
                keeper.finishedAtStart = true
                keeper.owesPushRemoval = true
                keeper.readsSlot = false
                var left: Left = []
                if !(slot?.delete() ?? true) { left.insert(.keychain) }
                if !record.retireStore() {
                    left.insert(.store)
                    keeper.storesInDoubt = true
                }
                left.formUnion(keeper.clearKept())
                if !record.keep(person: nil) { left.insert(.person) }
                SystemAppNotifications().signedOut()
                if left.isEmpty {
                    record.unmarkWipe()
                } else {
                    keeper.standing = Standing(session: marked, left: left)
                }
                log.notice("a sign-out the last run began was finished at this start: \(left.isEmpty ? "all of it" : "not all of it yet", privacy: .public)")
            } else {
                // A later session has written to the device since that
                // sign-out began: the mark is its own session's, and ends no other.
                record.unmarkWipe()
            }
        }
        keeper.person = record?.person()
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
        jar.mayKeep = { [weak self] in self?.passMark(savingCookies: false) ?? true }
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
        guard let said = underWay, said.object == nil else { return }
        underWay = nil
        unmark()
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
        guard takeAway(by: window, sparing: true) else { return }
        window.tail = .awaited
        underWay = Weak(window)
    }

    /// A window's core says the session is forgotten (`forget_session`).
    /// A core says it once for each time it said the session was ending:
    /// when the token's removal is answered, or in answer to being told that
    /// the session ended meanwhile. Either way the session it speaks of was
    /// ended when it said it was ending, and what came after is not its to
    /// end: the word is counted and passed over, nothing is taken away
    /// twice, and the mark goes if it waited for this window and all of it
    /// finished. Otherwise (a sign-out said in one word) the session ends
    /// here, for every window. `ender` is the window whose core said so: it
    /// goes on in the session that follows, and every other window's core is
    /// told. `true` when the session ended here, `false` when it had ended
    /// already.
    @discardableResult
    func end(by ender: SystemEffects? = nil) -> Bool {
        if let ender, ender.tail != .none {
            ender.tail = .none
            passedOver += 1
            if let said = underWay, said.object === ender {
                underWay = nil
                unmark()
            }
            return false
        }
        guard takeAway(by: ender, sparing: false) else {
            passedOver += 1
            return false
        }
        unmark()
        return true
    }

    /// A window's core asks for something new. One that was told the
    /// session ended while its own sign-out waited, and has not said the
    /// session is forgotten by now, never will for that sign-out: the next
    /// time it says so is a sign-out of its own.
    func asks(by window: SystemEffects) {
        if window.tail == .told { window.tail = .none }
    }

    /// Tells a window's core that the session ended in another window.
    private func tell(_ window: SystemEffects) {
        if window.tail == .awaited { window.tail = .told }
        window.sessionEnded()
    }

    /// Takes away everything of the session on this device, for every
    /// window, and moves the session's number on. With `sparing`, the
    /// requests of `ender` itself are left to be answered. The mark it
    /// writes names the session it ends, and stands until ``unmark()``.
    /// `false`, and nothing done, when it is asked for from inside itself (a
    /// core that answers being told with a sign-out of its own): what is
    /// under way is already taking everything away, for every window.
    @discardableResult
    private func takeAway(by ender: SystemEffects?, sparing: Bool) -> Bool {
        guard !taking else { return false }
        taking = true
        defer { taking = false }
        underWay = nil
        number += 1
        let session = record?.serial() ?? 0
        let marked = record?.markWipe(ending: session) ?? false
        let windows = live()
        for window in windows where !(sparing && window === ender) { window.stopWork() }
        // What could bring the session back: the cookies, then the web view's store.
        var left: Left = []
        var jars: [CookieJar] = []
        for jar in [made] + windows.map({ $0.transport.jar }) {
            if let jar, !jars.contains(where: { $0 === jar }) { jars.append(jar) }
        }
        for jar in jars {
            if !jar.forget() { left.insert(.keychain) }
        }
        if made == nil, let slot, !slot.delete() { left.insert(.keychain) }
        let old = web?.store
        web = nil
        if !(record?.retireStore() ?? true) {
            left.insert(.store)
            storesInDoubt = true
        }
        // Then everything else kept of the person, and who they were.
        left.formUnion(clearKept())
        person = nil
        if !(record?.keep(person: nil) ?? true) { left.insert(.person) }
        for window in windows { window.pictures.forget() }
        if let teller = ender ?? windows.first {
            teller.devices.notifications.signedOut()
        } else {
            SystemAppNotifications().signedOut()
        }
        // The mark stands from here: what the windows write as they start
        // again is the next session's, and goes through ``keeps()``.
        standing = marked ? Standing(session: session, left: left) : nil
        for window in windows where window !== ender { tell(window) }
        ender?.caughtUp()
        removeRetired(emptying: old)
        return true
    }

    /// Takes the mark of a sign-out away, once nothing of its session is
    /// left on the device. While something is, the mark stands: it is tried
    /// again when the next session first has something to write
    /// (``keeps()``), and at the next start.
    private func unmark() {
        guard let mark = standing, mark.left.isEmpty else { return }
        record?.unmarkWipe()
        standing = nil
    }

    // MARK: - What the session after a sign-out may write

    /// Whether something of the session as it is now may be written to the
    /// device: asked before a cookie goes into the Keychain, a page or what
    /// is kept of a window is written, a web view store is named. It may,
    /// unless the mark of a sign-out stands and names the session the device
    /// still holds. Then what that sign-out could not take away is tried
    /// again, and once it is gone the record's count moves past the mark
    /// before anything is written; until then the session is kept in memory
    /// only. So a start that finds a mark naming the device's session finds
    /// nothing of a later one beside it.
    func keeps() -> Bool {
        passMark(savingCookies: true)
    }

    private func passMark(savingCookies: Bool) -> Bool {
        guard var mark = standing, !mark.passed, let record else { return true }
        if !mark.left.isEmpty {
            mark.left = takeLeft(mark.left)
            standing = mark
            guard mark.left.isEmpty else { return false }
        }
        guard record.move(past: mark.session) else { return false }
        mark.passed = true
        standing = mark
        // A mark no core's word is still waited for has nothing more to say.
        if underWay?.object == nil {
            underWay = nil
            unmark()
        }
        // What the session held in memory meanwhile is written now.
        if savingCookies { made?.keep() }
        return true
    }

    /// Tries again to take off the device what a sign-out could not: what
    /// is still left afterwards.
    private func takeLeft(_ left: Left) -> Left {
        var still: Left = []
        if left.contains(.keychain) {
            // Only what the Keychain holds goes: the cookies in memory are
            // the session's as it is now.
            let gone = made.map { $0.unkeep() } ?? slot?.delete() ?? true
            if !gone { still.insert(.keychain) }
        }
        if left.contains(.store) {
            if record?.retireStore() ?? true {
                storesInDoubt = false
            } else {
                still.insert(.store)
            }
        }
        if left.contains(.pages), !cache.clear() { still.insert(.pages) }
        if left.contains(.places), !snapshots.clear() { still.insert(.places) }
        if left.contains(.files), !files.clear() { still.insert(.files) }
        if left.contains(.person), !(record?.keep(person: nil) ?? true) { still.insert(.person) }
        return still
    }

    // MARK: - Who is signed in

    /// Who a core says is signed in, as far as a client tells one person
    /// from another, and nothing that says who they are.
    struct Person: Equatable, Codable, Sendable {
        /// The key the core gives for the person: the same for one person,
        /// another for another account. None from a core that gives none.
        var key: String?
        /// The mark made from the name and the address the core names them
        /// by (``SessionKeeper/mark(name:email:)``).
        var mark: String

        /// Whether this is the person `other` is. By the core's key when
        /// both have one: somebody who changes their name is the same
        /// person, and two people of one name are two. By the mark of name
        /// and address when either has none: so with a core that gives no
        /// key, and the first time a key arrives for a person written down
        /// without one, which by itself changes nothing.
        func isSame(as other: Person) -> Bool {
            if let key, let theirs = other.key { return key == theirs }
            return mark == other.mark
        }
    }

    /// The mark of a person, made from the name and the address a core
    /// names them by: the same for the same two, another for any other, and
    /// neither can be read from it. A person with no address and one whose
    /// address is empty are two. Where a core gives no key for the person,
    /// the name and the address are all it says of them, and the mark is how
    /// a client tells people apart: somebody who changes their name or
    /// address is then taken for another person once, and two people with
    /// the same name and address are one.
    nonisolated static func mark(name: String, email: String?) -> String {
        let written = String(name.utf16.count) + ":" + name + (email.map { "+" + $0 } ?? "-")
        return SHA256.hash(data: Data(written.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    /// A window's core names who is signed in. The first person named since
    /// the last sign-out is written down as the one what is kept is kept
    /// for. Somebody else than that (``Person/isSame(as:)``), with no
    /// sign-out between, is a change of person: a session ran out, and
    /// another person signed in, in this run or since the one before it.
    /// What was the last person's then goes for every window
    /// (``personChanged(by:)``), and who is written down is replaced only
    /// when all of it went: an app ended in the middle, or a part that
    /// failed, leaves the person before written down, and the next start
    /// does the change over.
    func hears(_ named: Person, from window: SystemEffects) {
        guard let before = person else {
            person = named
            if keeps() { _ = record?.keep(person: named) }
            return
        }
        var now = named
        // The person written down with a key, named once without one, keeps it.
        if now.key == nil, before.isSame(as: now) { now.key = before.key }
        guard before != now else { return }
        person = now
        // The same person, named anew (a key where there was none, another
        // name under the same key), is only written down as they are now.
        if before.isSame(as: now) || personChanged(by: window) {
            if keeps() { _ = record?.keep(person: now) }
        }
    }

    /// The core of `window` names another person than the session's, and
    /// nobody signed out between. The cookies are the new person's already,
    /// and what a core keeps of pages it empties itself. Everything else
    /// that was the last person's goes here, for every window: the session's
    /// number moves on, so what was begun for them is refused; the other
    /// windows' requests are dropped and their cores told, as at a sign-out,
    /// so none of them shows the last person's screens or sends what they
    /// typed as the new one; the web view's store is retired; what was kept
    /// of each window, the files picked for forms, the pictures loaded, the
    /// notifications and the count on the app's icon go. `true` when all of
    /// it went.
    private func personChanged(by window: SystemEffects) -> Bool {
        number += 1
        let windows = live()
        for other in windows where other !== window { other.stopWork() }
        let old = web?.store
        web = nil
        var finished = true
        if !(record?.retireStore() ?? true) {
            storesInDoubt = true
            finished = false
        }
        finished = snapshots.clear() && finished
        finished = files.clear() && finished
        for each in windows { each.pictures.forget() }
        window.devices.notifications.signedOut()
        for other in windows where other !== window { tell(other) }
        window.follows()
        removeRetired(emptying: old)
        return finished
    }

    /// Whether a sign-out finished at this start still has the push token
    /// to give up: answered `true` once.
    func takesPushRemoval() -> Bool {
        defer { owesPushRemoval = false }
        return owesPushRemoval
    }

    /// Deletes the pages, what was kept of each window, and the picked
    /// files: what of them is still on the device afterwards.
    private func clearKept() -> Left {
        var left: Left = []
        if !cache.clear() { left.insert(.pages) }
        if !snapshots.clear() { left.insert(.places) }
        if !files.clear() { left.insert(.files) }
        return left
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
        guard let record, record.keepsWebStore, !storesInDoubt, keeps() else { return nil }
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
/// that no backup takes: that a sign-out has begun and not finished, and of
/// which session; the number of the session the device holds; the mark of
/// the person what is kept is kept for; and which web view store is the
/// session's and which belong to sessions that ended. Nothing in it is a
/// secret, and it can be read whenever the Keychain item can (after the
/// device's first unlock).
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
    private var count: URL { folder.appendingPathComponent("sessions") }
    private var who: URL { folder.appendingPathComponent("person") }

    /// The number of the session the device holds, counted across launches.
    /// It moves on when a session first writes to the device while the mark
    /// of the sign-out before it still stands (``move(past:)``), and at no
    /// other time: it is what a mark names its session by. 0 until then.
    func serial() -> Int {
        guard let data = try? Data(contentsOf: count) else { return 0 }
        return Int(String(decoding: data, as: UTF8.self)) ?? 0
    }

    /// Moves the device's session number past `session`: a later session is
    /// about to write to the device. `false` when that could not be written
    /// down, and then nothing of the later session may be written either.
    func move(past session: Int) -> Bool {
        do {
            try prepare()
            try Data(String(session + 1).utf8).write(
                to: count, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication]
            )
            return true
        } catch {
            return false
        }
    }

    /// The session the mark of a sign-out names, when one stands. A mark
    /// that does not read names the session the device holds.
    func marked() -> Int? {
        guard FileManager.default.fileExists(atPath: mark.path) else { return nil }
        guard let data = try? Data(contentsOf: mark) else { return serial() }
        return Int(String(decoding: data, as: UTF8.self)) ?? serial()
    }

    /// Whether a sign-out began and did not finish, and the session it ends
    /// is still the one the device holds: a mark the count has moved past is
    /// an earlier session's, and owes nothing.
    var owesWipe: Bool {
        guard let marked = marked() else { return false }
        return marked >= serial()
    }

    /// Writes down that the sign-out of `session` (the device's own, when
    /// none is given) has begun, and has the system put it on the disk before
    /// anything is deleted. `false` when it could not be.
    func markWipe(ending session: Int? = nil) -> Bool {
        do {
            try prepare()
            try Data(String(session ?? serial()).utf8).write(
                to: mark, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication]
            )
            let handle = try FileHandle(forWritingTo: mark)
            try handle.synchronize()
            try handle.close()
            return true
        } catch {
            return false
        }
    }

    /// Takes the mark away: the sign-out finished, or its session is not the
    /// one the device holds any more.
    @discardableResult
    func unmarkWipe() -> Bool {
        try? FileManager.default.removeItem(at: mark)
        return !FileManager.default.fileExists(atPath: mark.path)
    }

    /// The person what is kept on the device is kept for, by the core's key
    /// and the mark of their name and address (``SessionKeeper/Person``);
    /// none when nobody was named since the last sign-out.
    func person() -> SessionKeeper.Person? {
        guard let data = try? Data(contentsOf: who) else { return nil }
        return try? JSONDecoder().decode(SessionKeeper.Person.self, from: data)
    }

    /// Writes down whose what is kept is, or with nobody that it is
    /// nobody's. `false` when it could not be.
    func keep(person: SessionKeeper.Person?) -> Bool {
        guard let person else {
            try? FileManager.default.removeItem(at: who)
            return !FileManager.default.fileExists(atPath: who.path)
        }
        do {
            try prepare()
            try JSONEncoder().encode(person).write(
                to: who, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication]
            )
            return true
        } catch {
            return false
        }
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
