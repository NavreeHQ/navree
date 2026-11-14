import Foundation
import NavreeProtocol

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
