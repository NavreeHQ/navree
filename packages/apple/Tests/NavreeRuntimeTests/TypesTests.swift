import Foundation
import NavreeProtocol
import Testing

@testable import NavreeRuntime

/// The core's app part names the person signed in, and gives a key for them
/// when it has one: the same for one person, another for another account.
@Test func theAppPartReadsThePersonsKeyWhenTheCoreGivesOneAndNoneOtherwise() throws {
    let keyed = try JSONDecoder().decode(
        AppPart.self,
        from: Data(#"{"name":"Charkha","home":"/","user":{"name":"Harpreet Singh","key":"person-1f0c"}}"#.utf8)
    )
    #expect(keyed.user?.name == "Harpreet Singh")
    #expect(keyed.user?.key == "person-1f0c")
    // A core that gives no key says the person as before.
    let plain = try JSONDecoder().decode(
        AppPart.self, from: Data(#"{"name":"Charkha","home":"/","user":{"name":"Harpreet Singh"}}"#.utf8)
    )
    #expect(plain.user?.name == "Harpreet Singh")
    #expect(plain.user?.key == nil)
    // With nobody signed in there is no person, and no key.
    let nobody = try JSONDecoder().decode(AppPart.self, from: Data(#"{"name":"Charkha","home":"/"}"#.utf8))
    #expect(nobody.user == nil)
    #expect(keyed.userWas == nil)
}

/// To a client that says it still keeps a plain key, the core says beside
/// the person what their plain key was; to any other it says none.
@Test func theAppPartReadsWhatThePersonsKeyWasWhenTheCoreSaysIt() throws {
    let said = try JSONDecoder().decode(
        AppPart.self,
        from: Data(
            #"{"name":"Charkha","home":"/","user":{"name":"Ravneet Kaur","key":"k2:17d27bf8"},"user_was":"1c54d473"}"#.utf8
        )
    )
    #expect(said.user?.key == "k2:17d27bf8")
    #expect(said.userWas == "1c54d473")
    let unsaid = try JSONDecoder().decode(
        AppPart.self, from: Data(#"{"name":"Charkha","home":"/","user":{"name":"Ravneet Kaur","key":"k2:17d27bf8"}}"#.utf8)
    )
    #expect(unsaid.userWas == nil)
}

/// Events are written as the core's own conversations write them.
@Test func eventsAreWrittenAsTheCoreReadsThem() throws {
    let cases: [(CoreEvent, [String: Any])] = [
        (.start, ["t": "start"]),
        (.select(destination: 1), ["t": "select", "destination": 1]),
        (
            .act(.init(screen: 3, action: .submit(.init()), form: "refund", overlay: "refund")),
            ["t": "act", "screen": 3, "action": ["t": "submit"], "form": "refund", "overlay": "refund"]
        ),
        (
            .act(.init(screen: 2, action: .navigate(.init(href: "/orders/1/returns/new", present: .task)))),
            ["t": "act", "screen": 2, "action": ["t": "navigate", "href": "/orders/1/returns/new", "present": "task"]]
        ),
        (
            .field(screen: 3, form: "refund", field: "refund-amount", value: .text("0")),
            ["t": "field", "screen": 3, "form": "refund", "field": "refund-amount", "value": ["t": "text", "text": "0"]]
        ),
        (
            .field(screen: 1, form: nil, field: "notify", value: .flag(true)),
            ["t": "field", "screen": 1, "field": "notify", "value": ["t": "flag", "on": true]]
        ),
        (
            .field(screen: 1, form: "f", field: "when", value: .range(from: "2026-11-01", to: "")),
            ["t": "field", "screen": 1, "form": "f", "field": "when", "value": ["t": "range", "from": "2026-11-01", "to": ""]]
        ),
        (.answer(screen: 1, question: 2, yes: true), ["t": "answer", "screen": 1, "question": 2, "yes": true]),
        (.more(screen: 3, id: "feed", before: false), ["t": "more", "screen": 3, "id": "feed"]),
        (.more(screen: 3, id: "feed", before: true), ["t": "more", "screen": 3, "id": "feed", "before": true]),
        (.dismissed(screen: 3), ["t": "dismissed", "screen": 3]),
        (.popTo(screen: 2), ["t": "pop_to", "screen": 2]),
        (.overlay(screen: 3, id: "refund", open: false), ["t": "overlay", "screen": 3, "id": "refund", "open": false]),
        (.tab(screen: 1, id: "parts", index: 1), ["t": "tab", "screen": 1, "id": "parts", "index": 1]),
        (.random(request: 1, bytes: "0a1b"), ["t": "random", "request": 1, "bytes": "0a1b"]),
        (.pushToken(token: "ab12", sandbox: true), ["t": "push_token", "token": "ab12", "environment": "sandbox"]),
        (.window(width: .expanded), ["t": "window", "width": "expanded"]),
        (
            .environment(.init(scheme: .dark, textPercent: 130)),
            ["t": "environment", "scheme": "dark", "text_percent": 130]
        ),
        (.stream(stream: 1, state: .event(name: "orders")), ["t": "stream", "stream": 1, "state": "event", "name": "orders"]),
        (.stream(stream: 1, state: .closed(status: 403)), ["t": "stream", "stream": 1, "state": "closed", "status": 403]),
        (.cache(key: "boot", value: nil), ["t": "cache", "key": "boot"]),
        (
            .deviceResult(call: 1, outcome: .done([Navree.Pair(name: "completed", value: "true")])),
            ["t": "device_result", "call": 1, "ok": true, "fields": [["name": "completed", "value": "true"]]]
        ),
        (
            .deviceResult(call: 1, outcome: .refused(.cancelled)),
            ["t": "device_result", "call": 1, "ok": false, "refusal": "cancelled"]
        ),
        (.signInDone(url: nil), ["t": "sign_in_done"]),
        (
            .signInDone(url: "https://shop.example/app/signed-in?code=c&state=s"),
            ["t": "sign_in_done", "url": "https://shop.example/app/signed-in?code=c&state=s"]
        ),
        (
            .response(Response(
                request: 5, status: 303, location: "/orders/1", headers: [["x-navree-shell", "9f3c"]]
            )),
            ["t": "response", "request": 5, "status": 303, "location": "/orders/1", "headers": [["x-navree-shell", "9f3c"]]]
        ),
        (.response(.failed(5, .offline)), ["t": "response", "request": 5, "error": "offline"]),
        (.focused(screen: 3, serial: 1), ["t": "focused", "screen": 3, "serial": 1]),
    ]
    for (event, expected) in cases {
        let written = try #require(try JSONSerialization.jsonObject(with: event.json) as? NSDictionary)
        #expect(written == expected as NSDictionary, "\(String(decoding: event.json, as: UTF8.self))")
    }
}

@Test func aRestoreCarriesTheSnapshotExactlyAsItWasKept() throws {
    let snapshot = Data(#"{"v":1,"origin":"https://shop.example","n":18446744073709551615}"#.utf8)
    let written = String(decoding: CoreEvent.restore(snapshot: snapshot).json, as: UTF8.self)
    #expect(written == #"{"t":"restore","snapshot":{"v":1,"origin":"https://shop.example","n":18446744073709551615}}"#)
}

@Test func aControlsValuesKeepTheShapeTheCoreKeepsForItsField() {
    #expect(FieldValue.flag(false).replaced(with: ["on"]) == .flag(true))
    #expect(FieldValue.flag(true).replaced(with: []) == .flag(false))
    #expect(FieldValue.many(["a"]).replaced(with: ["a", "b"]) == .many(["a", "b"]))
    #expect(FieldValue.text("").replaced(with: ["10"]) == .text("10"))
    #expect(FieldValue.none.replaced(with: ["late"]) == .text("late"))
    #expect(FieldValue.none.replaced(with: []) == .none)
    #expect(FieldValue.phone(text: "", country: "IN").replaced(with: ["98140 12345"]) == .phone(text: "98140 12345", country: "IN"))
    #expect(FieldValue.range(from: "", to: "").replaced(with: ["2026-11-01", "2026-11-12"]) == .range(from: "2026-11-01", to: "2026-11-12"))
    #expect(FieldValue.flag(true).values == ["on"])
}

@Test func aWordOfAClosedListFromANewerCoreIsReadAsItsDefault() throws {
    let modal = try JSONDecoder().decode(Modal.self, from: Data(#"{"present":"hologram","locked":true,"stack":[4]}"#.utf8))
    #expect(modal.present == .sheet)
    #expect(modal.locked)
    let nav = try JSONDecoder().decode(NavPart.self, from: Data(#"{"stage":"warping","window":{"width":"vast"},"selected":0}"#.utf8))
    #expect(nav.stage == .starting)
    #expect(nav.width == .compact)
    let message = try JSONDecoder().decode(Message.self, from: Data(#"{"id":3,"tone":"glitter","notice":"tired"}"#.utf8))
    #expect(message.tone == .neutral)
    // A notice this build does not know is still a notice, never none.
    #expect(message.notice == .unknown)
}

/// The observed model is never compared value by value: a screen's tree may
/// be as deep as the protocol allows, and the comparison would run on the
/// main thread each time a screen is put in place.
@Test func theViewStateIsNeverComparedValueByValue() {
    func equatable(_ type: Any.Type) -> Bool { type is any Equatable.Type }
    #expect(!equatable(ScreenState.self))
    #expect(!equatable(ScreenContent.self))
    #expect(!equatable(NavPart.self))
    #expect(!equatable(AppPart.self))
}

/// The core's notices are read by its words for them.
@Test func aNoticeANewerCoreSaysIsStillANotice() throws {
    let messages = try JSONDecoder().decode([Message].self, from: Data("""
        [{"id":1,"tone":"warning","notice":"saved_not_refreshed"},
         {"id":2,"tone":"danger","notice":"said_by_a_newer_core"},
         {"id":3,"tone":"neutral","text":"Packed"},
         {"id":4,"tone":"danger","notice":"failed"}]
        """.utf8))
    #expect(messages.map(\.notice) == [.savedNotRefreshed, .unknown, nil, .failed])
    #expect(CoreNotice.read("saved_not_refreshed") == .savedNotRefreshed)
    #expect(CoreNotice.read("said_by_a_newer_core") == .unknown)
    guard case .announce(_, let notice, _) = try CoreEffect.read(["t": "announce", "notice": "said_by_a_newer_core"]) else {
        Issue.record("not an announcement")
        return
    }
    #expect(notice == .unknown)
}

/// An effect a newer core says is passed over, and the effects beside it are read.
@Test func anEffectThisBuildDoesNotKnowIsPassedOverAndTheRestAreRead() throws {
    let update = try Update.read(Data(#"""
        {"changed":[],"effects":[{"t":"from_a_newer_core","with":[1,2]},{"t":"session_ending"},{"t":"forget_session"},{"t":"cancel"}]}
        """#.utf8)).get()
    #expect(update.effects.count == 3)
    guard case .unknown(let tag) = update.effects[0], case .sessionEnding = update.effects[1],
        case .forgetSession = update.effects[2]
    else {
        Issue.record("not read as an unknown effect, the session ending and the session forgotten")
        return
    }
    #expect(tag == "from_a_newer_core")
    // An effect this build knows that does not read (a cancel that names no request) is said, never carried out.
    #expect(update.unread == ["cancel"])
}
