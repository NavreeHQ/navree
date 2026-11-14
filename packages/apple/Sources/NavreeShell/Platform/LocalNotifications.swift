import Foundation
import NavreeProtocol
import UserNotifications
import os

/// When a scheduled notification shows, in the system's terms.
public enum NotifyTrigger: Sendable, Equatable {
    /// On a day at a time (`UNCalendarNotificationTrigger`, once): the
    /// moment's date and time in the device's zone.
    case date(DateComponents)
    /// After a while (`UNTimeIntervalNotificationTrigger`), in seconds.
    case after(TimeInterval)
    /// Every day at an hour and minute of the device's zone
    /// (`UNCalendarNotificationTrigger`, repeating).
    case daily(hour: Int, minute: Int)
}

/// Notifications the device schedules itself, as plain rules.
public enum NotifyRules {
    /// When a notification shows: a moment in the device's zone (at once
    /// when it has passed), a wait of at least one second (the system's
    /// least), every day at a valid hour and minute. A time this build does
    /// not know, from a newer server, is not scheduled, as the protocol says.
    public static func trigger(_ when: Navree.When, timeZone: TimeZone, now: Date = Date()) -> NotifyTrigger? {
        switch when {
        case .at(let at):
            let moment = Date(timeIntervalSince1970: TimeInterval(at.unix))
            guard moment > now else { return .after(1) }
            var calendar = Calendar(identifier: .gregorian)
            calendar.timeZone = timeZone
            var parts = calendar.dateComponents([.year, .month, .day, .hour, .minute, .second], from: moment)
            parts.calendar = calendar
            parts.timeZone = timeZone
            return .date(parts)
        case .after(let after):
            return .after(TimeInterval(max(1, after.secs)))
        case .daily(let daily):
            guard (0...23).contains(daily.hour), (0...59).contains(daily.minute) else { return nil }
            return .daily(hour: daily.hour, minute: daily.minute)
        case .unknown:
            return nil
        }
    }

    /// What a notification carries for its tap: its address, when it is a
    /// path of the app's own origin; a tap then opens it as any link from
    /// outside does.
    public static func userInfo(href: String?) -> [String: String] {
        guard let href, let path = OpeningRules.address(href) else { return [:] }
        return ["href": path]
    }
}

/// What carries out the app's own notifications and the count on its icon:
/// the system, or a stand-in in tests.
@MainActor
protocol AppNotifications: AnyObject {
    /// Schedules a notification, asking the person first the first time.
    func schedule(_ notify: Navree.Notify, permission: any NotificationPermission)
    /// Takes back a scheduled or shown notification.
    func cancel(_ id: String)
    /// Sets the count on the app's icon, or clears it.
    func badge(_ count: Int?)
    /// The person signed out: the count and every notification of theirs go.
    func signedOut()
}

/// The system's own notification center (`UNUserNotificationCenter`).
@MainActor
final class SystemAppNotifications: AppNotifications {
    nonisolated static let log = Logger(subsystem: "com.navree.native", category: "notifications")

    func schedule(_ notify: Navree.Notify, permission: any NotificationPermission) {
        guard PresenceModel.isApp else { return }
        guard let trigger = NotifyRules.trigger(notify.at, timeZone: .current) else {
            return Self.log.notice("a notification at a time this build does not know is not scheduled")
        }
        permission.ask { granted in
            guard granted else { return }
            let content = UNMutableNotificationContent()
            content.title = notify.title
            if let body = notify.body { content.body = body }
            if let thread = notify.thread { content.threadIdentifier = thread }
            content.userInfo = NotifyRules.userInfo(href: notify.href)
            content.sound = .default
            let request = UNNotificationRequest(identifier: notify.id, content: content, trigger: Self.system(trigger))
            UNUserNotificationCenter.current().add(request) { error in
                if let error { Self.log.error("a notification was not scheduled: \(error.localizedDescription, privacy: .public)") }
            }
        }
    }

    func cancel(_ id: String) {
        guard PresenceModel.isApp else { return }
        let center = UNUserNotificationCenter.current()
        center.removePendingNotificationRequests(withIdentifiers: [id])
        center.removeDeliveredNotifications(withIdentifiers: [id])
    }

    func badge(_ count: Int?) {
        PresenceModel.shared.call(badge: count)
    }

    func signedOut() {
        PresenceModel.shared.call(badge: nil)
        guard PresenceModel.isApp else { return }
        let center = UNUserNotificationCenter.current()
        center.removeAllPendingNotificationRequests()
        center.removeAllDeliveredNotifications()
    }

    /// The trigger as the system writes it.
    static func system(_ trigger: NotifyTrigger) -> UNNotificationTrigger {
        switch trigger {
        case .date(let parts):
            UNCalendarNotificationTrigger(dateMatching: parts, repeats: false)
        case .after(let seconds):
            UNTimeIntervalNotificationTrigger(timeInterval: max(1, seconds), repeats: false)
        case .daily(let hour, let minute):
            UNCalendarNotificationTrigger(dateMatching: DateComponents(hour: hour, minute: minute), repeats: true)
        }
    }
}
