import Foundation
import NavreeProtocol
import Testing

@testable import NavreeShell

private let kolkata = TimeZone(identifier: "Asia/Kolkata")!
/// 14 November 2026, 09:00 in Kolkata.
private let now = Date(timeIntervalSince1970: 1_794_627_000)

@Test func aMomentIsADateInTheDevicesZone() throws {
    // 15 November 2026, 18:30 in Kolkata (13:00 UTC).
    let trigger = try #require(NotifyRules.trigger(.at(.init(unix: 1_794_747_600)), timeZone: kolkata, now: now))
    guard case .date(let parts) = trigger else { Issue.record("not a date: \(trigger)"); return }
    #expect(parts.year == 2026)
    #expect(parts.month == 11)
    #expect(parts.day == 15)
    #expect(parts.hour == 18)
    #expect(parts.minute == 30)
    #expect(parts.timeZone == kolkata)
}

@Test func aMomentAlreadyPastIsShownAtOnce() {
    #expect(NotifyRules.trigger(.at(.init(unix: 1_700_000_000)), timeZone: kolkata, now: now) == .after(1))
}

@Test func afterNoTimeStillWaitsOneSecond() {
    #expect(NotifyRules.trigger(.after(.init(secs: 0)), timeZone: kolkata) == .after(1))
    #expect(NotifyRules.trigger(.after(.init(secs: 90)), timeZone: kolkata) == .after(90))
}

@Test func dailyRepeatsAtItsHourAndMinute() {
    #expect(NotifyRules.trigger(.daily(.init(hour: 7, minute: 45)), timeZone: kolkata) == .daily(hour: 7, minute: 45))
    #expect(NotifyRules.trigger(.daily(.init(hour: 24, minute: 0)), timeZone: kolkata) == nil)
    #expect(NotifyRules.trigger(.daily(.init(hour: 9, minute: 60)), timeZone: kolkata) == nil)
}

@Test func aTimeFromANewerServerIsNotScheduled() throws {
    let when = try JSONDecoder().decode(Navree.When.self, from: Data(#"{"t":"weekly","day":2}"#.utf8))
    #expect(NotifyRules.trigger(when, timeZone: kolkata) == nil)
}

@Test func aNotificationCarriesItsAddressForATap() {
    #expect(NotifyRules.userInfo(href: "/orders/1042") == ["href": "/orders/1042"])
    #expect(NotifyRules.userInfo(href: nil).isEmpty)
    #expect(NotifyRules.userInfo(href: "https://elsewhere.example/").isEmpty)
    #expect(NotifyRules.userInfo(href: "//elsewhere.example/").isEmpty)
}

/// The app's notifications and its count, kept as they are asked for.
@MainActor
final class PlayedNotifications: AppNotifications {
    var scheduled: [String] = []
    var cancelled: [String] = []
    var badges: [Int?] = []
    var signOuts = 0

    func schedule(_ notify: Navree.Notify, permission: any NotificationPermission) {
        permission.ask { granted in if granted { self.scheduled.append(notify.id) } }
    }

    func cancel(_ id: String) { cancelled.append(id) }
    func badge(_ count: Int?) { badges.append(count) }
    func signedOut() { signOuts += 1 }
}

@MainActor
@Test func theBadgeANotificationAndItsCancellingAreCarriedOutAndNeverAnswered() {
    let calls = DeviceCalls(signedWith: [], origin: nil)
    let played = PlayedNotifications()
    calls.notifications = played
    calls.permission = PlayedPermission(granted: true)
    var answers: [Int] = []
    calls.answer = { call, _ in answers.append(call) }
    // From a server's reply, and from a tap.
    calls.run(call: 1, device: .reply(.setBadge(.init(count: 3))), posts: false, options: nil)
    calls.run(call: 2, device: .call(.setBadge(.init())), posts: false, options: nil)
    let notify = Navree.Notify(at: .after(.init(secs: 60)), href: "/orders/1042", id: "order-1042", title: "Order 1042 is packed")
    calls.run(call: 3, device: .call(.notify(notify)), posts: false, options: nil)
    calls.run(call: 4, device: .reply(.cancelNotification(.init(id: "order-1042"))), posts: false, options: nil)
    calls.run(call: 5, device: .call(.cancelNotification(.init(id: "order-7"))), posts: false, options: nil)
    #expect(played.badges == [3, nil])
    #expect(played.scheduled == ["order-1042"])
    #expect(played.cancelled == ["order-1042", "order-7"])
    #expect(answers.isEmpty)
}

@MainActor
@Test func aNotificationThePersonDidNotAllowIsNotScheduled() {
    let calls = DeviceCalls(signedWith: [], origin: nil)
    let played = PlayedNotifications()
    calls.notifications = played
    calls.permission = PlayedPermission(granted: false)
    calls.run(call: 1, device: .call(.notify(.init(at: .after(.init(secs: 5)), id: "n", title: "Chai break"))), posts: false, options: nil)
    #expect(played.scheduled.isEmpty)
}

@MainActor
@Test func signingOutClearsTheBadgeAndTheNotifications() throws {
    let shell = try Shell()
    let played = PlayedNotifications()
    shell.effects.devices.notifications = played
    shell.effects.run(.forgetSession, for: shell.client)
    #expect(played.signOuts == 1)
}

@MainActor
@Test func aFollowedDestinationSetsTheCountOfTheWholeApp() {
    let model = PresenceModel()
    model.configure(Presence(badge: .destination("/inbox")))
    model.heard(scene: "A", destinations: [PresenceDestination(label: "Inbox", href: "/inbox", badge: "7")], user: nil)
    // Heard before the window registered: nothing is in front yet.
    #expect(model.badge.count == nil)
    let session = Session(configuration: ShellConfiguration(origin: "https://charkha.example", appVersion: "1"))
    model.attach(scene: "A", session: session)
    model.heard(scene: "A", destinations: [PresenceDestination(label: "Inbox", href: "/inbox", badge: "7")], user: nil)
    #expect(model.front == "A")
    #expect(model.badge.count == 7)
    model.call(badge: 2)
    #expect(model.badge.count == 2)
}
