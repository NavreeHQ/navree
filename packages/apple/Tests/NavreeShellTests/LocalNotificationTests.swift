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
