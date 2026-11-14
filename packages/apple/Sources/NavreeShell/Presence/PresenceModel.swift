import Foundation
import NavreeProtocol
import Observation
import UserNotifications
import os

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// What the app's place in the system needs of its windows, once for the
/// process: the destinations of the window in front, the count on the app's
/// icon, the windows themselves. All of it mirrors what the windows' cores
/// hold or what the system says: where anything leads stays the cores' to say.
@MainActor
@Observable
public final class PresenceModel {
    /// The one model of the process.
    public static let shared = PresenceModel()

    nonisolated static let log = Logger(subsystem: "com.navree.native", category: "presence")

    /// The app's presence, as its configuration says.
    public private(set) var presence = Presence()
    /// The window in front's destinations.
    public private(set) var destinations: [PresenceDestination] = []
    /// The person signed in, in the window in front.
    public private(set) var user: Navree.User?
    /// The count on the app's icon, and what set it.
    public private(set) var badge = BadgeRules.State()
    /// The window in front, by its scene's name.
    public private(set) var front: String?

    /// Each window's session, by its scene's name.
    @ObservationIgnored var sessions: [String: Weak<Session>] = [:]
    /// The windows' names, the one shown last at the end.
    @ObservationIgnored var order: [String] = []
    /// What each window last heard of its destinations and its person.
    @ObservationIgnored private var heard: [String: (destinations: [PresenceDestination], user: Navree.User?)] = [:]

    init() {}

    /// The app's name, as the system shows it in its menus.
    public static var appName: String {
        let info = Bundle.main.infoDictionary ?? [:]
        return info["CFBundleDisplayName"] as? String ?? info["CFBundleName"] as? String
            ?? ProcessInfo.processInfo.processName
    }

    /// The person's choice of place, as the system's defaults keep it.
    public static var storedPlace: MacPlace? {
        UserDefaults.standard.string(forKey: PresenceRules.placeKey).flatMap(MacPlace.init(rawValue:))
    }

    /// Takes the app's configuration; the first one given stands.
    func configure(_ presence: Presence) {
        guard !configured else { return }
        configured = true
        self.presence = presence
        receivedConfiguration()
    }

    @ObservationIgnored private var configured = false

    // MARK: - The windows' destinations

    /// A window began, or took another name.
    func attach(scene: String, session: Session) {
        sessions[scene] = Weak(session)
        if !order.contains(scene) { order.append(scene) }
        if front == nil { show(scene) }
    }

    /// A window went.
    func detach(scene: String) {
        sessions[scene] = nil
        heard[scene] = nil
        order.removeAll { $0 == scene }
        if front == scene { front = nil }
    }

    /// A window came in front.
    func show(_ scene: String) {
        guard sessions[scene] != nil else { return }
        front = scene
        order.removeAll { $0 == scene }
        order.append(scene)
        if let mirror = heard[scene] { publish(mirror.destinations, user: mirror.user) }
    }

    /// What a window's core now holds of its destinations and its person.
    func heard(scene: String, destinations: [PresenceDestination], user: Navree.User?) {
        heard[scene] = (destinations, user)
        if front == nil { show(scene) }
        if front == scene { publish(destinations, user: user) }
    }

    private func publish(_ destinations: [PresenceDestination], user: Navree.User?) {
        if self.destinations != destinations { self.destinations = destinations }
        if self.user != user { self.user = user }
        let followed = BadgeRules.destinationsChanged(badge, source: presence.badge, destinations: destinations)
        if followed != badge {
            badge = followed
            applyBadge()
        }
        destinationsChanged()
    }

    // MARK: - The count on the app's icon

    /// A server's `set_badge`: the count on the app's icon, or none.
    public func call(badge count: Int?) {
        badge = BadgeRules.called(badge, count: count)
        applyBadge()
    }

    /// The count, where the system shows it: the app's icon on the Home
    /// Screen, as the person's notification settings allow; the Dock icon on
    /// a Mac. The status item reads it from here.
    private func applyBadge() {
        guard Self.isApp else { return }
        #if canImport(UIKit)
        UNUserNotificationCenter.current().setBadgeCount(badge.count ?? 0) { error in
            if let error { Self.log.error("the badge was not set: \(error.localizedDescription, privacy: .public)") }
        }
        #elseif canImport(AppKit)
        NSApp.dockTile.badgeLabel = BadgeRules.label(badge.count)
        #endif
    }

    /// Whether the process is an app, whose icon and notifications the system
    /// serves: not a test runner.
    nonisolated static var isApp: Bool { Bundle.main.bundleURL.pathExtension == "app" }

    // MARK: - What the rest of the presence hears

    /// Where the configuration first arrives: the place, the item, the Dock.
    private func receivedConfiguration() {}

    /// Where a change of the window in front's destinations goes next.
    private func destinationsChanged() {}
}

/// A reference that does not keep what it refers to.
struct Weak<Object: AnyObject> {
    weak var object: Object?

    init(_ object: Object) {
        self.object = object
    }
}
