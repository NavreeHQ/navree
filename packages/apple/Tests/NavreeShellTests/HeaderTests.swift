import Foundation
import NavreeRuntime
import Testing

@testable import NavreeShell

@MainActor
struct HeaderTests {
    @Test func aHeaderValueOfAnotherScriptOrWithALineBreakIsAnsweredAsFailedAndNeverSent() async {
        let host = "headers-checked.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: [:], body: "{}"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        for value in ["ऑर्डर", "a\r\nx-other: 1", "a\nb", "a\u{0}b", "a\u{7F}b"] {
            let request = FetchRequest(request: 4, url: "https://\(host)/orders", headers: [("x-navree-region", value)])
            #expect(transport.urlRequest(for: request) == nil, "\(value.debugDescription)")
            #expect(await fetched(transport, request) == .failed(4, .other), "\(value.debugDescription)")
        }
        #expect(StubOrigin.seen(host).isEmpty)
        // Printable ASCII, a space and a tab go as they are.
        let plain = FetchRequest(request: 5, url: "https://\(host)/orders", headers: [("x-navree-region", "orders list\t2")])
        #expect(transport.urlRequest(for: plain)?.value(forHTTPHeaderField: "x-navree-region") == "orders list\t2")
    }

    @Test func aHeaderNameThatIsNotATokenIsNeverSent() {
        #expect(!HTTPTransport.areSendable([("x-navree: region", "a")]))
        #expect(!HTTPTransport.areSendable([("", "a")]))
        #expect(!HTTPTransport.areSendable([("x-नाम", "a")]))
        #expect(HTTPTransport.areSendable([("x-navree-region", "inbox%E0%A4%91"), ("accept", "application/json")]))
    }

    @Test func aTokenThatCannotGoAsAHeaderIsLeftOutAndNeverSentBroken() throws {
        let host = "cookie-checked.shop.example"
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        // A token that decodes to a line break: sent as a header it would write a header of its own.
        jar.merge([CookieRecord(name: "XSRF-TOKEN", value: "a%0D%0Ab", domain: host, httpOnly: false)])
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        // The write goes without the header, for the server to answer; the
        // token never goes as it decodes. (Such a token used to stop the
        // request: with a page's script able to set one, that stopped every
        // write under the cookie's path for as long as the cookie lived.)
        let write = try #require(transport.urlRequest(for: FetchRequest(request: 6, method: "post", url: "https://\(host)/x", xsrf: true)))
        #expect(write.value(forHTTPHeaderField: "x-xsrf-token") == nil)
        #expect(write.value(forHTTPHeaderField: "Cookie") == "XSRF-TOKEN=a%0D%0Ab")
        #expect(transport.urlRequest(for: FetchRequest(request: 7, url: "https://\(host)/x")) != nil)
    }

    @Test func aTokenThatDecodesToAnotherScriptDoesNotStopTheWritesUnderItsPath() throws {
        let host = "token-left-out.shop.example"
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        // The session's own token, and one a page's script set for the
        // account's pages: its value is plain, and decodes to Gurmukhi.
        jar.merge([
            CookieRecord(name: "XSRF-TOKEN", value: "eyJpdiI6%3D%3D", domain: host, httpOnly: false),
            CookieRecord(name: "XSRF-TOKEN", value: "%E0%A8%85", domain: host, path: "/account", httpOnly: false),
        ])
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let account = try #require(URL(string: "https://\(host)/account/name"))
        // The nearest token that can be sent is the one that goes.
        #expect(jar.xsrfToken(for: account) == "eyJpdiI6==")
        let write = try #require(transport.urlRequest(for: FetchRequest(request: 8, method: "post", url: account.absoluteString, xsrf: true)))
        #expect(write.value(forHTTPHeaderField: "x-xsrf-token") == "eyJpdiI6==")
        // With no other token, the write goes without the header rather than not at all.
        let alone = CookieJar(origin: "https://\(host)", persistent: false)
        alone.merge([CookieRecord(name: "XSRF-TOKEN", value: "%E0%A8%85", domain: host, path: "/account", httpOnly: false)])
        #expect(alone.xsrfToken(for: account) == nil)
        let bare = try #require(
            HTTPTransport(jar: alone, configuration: StubOrigin.configuration)
                .urlRequest(for: FetchRequest(request: 9, method: "post", url: account.absoluteString, xsrf: true))
        )
        #expect(bare.value(forHTTPHeaderField: "x-xsrf-token") == nil)
        #expect(bare.httpMethod == "POST")
    }

    @Test func aCookieThatCannotGoOnTheWireIsLeftOutAndEveryRequestStillGoes() async {
        let host = "cookie-left-out.shop.example"
        StubOrigin.answer(host, .init(status: 200, headers: [:], body: "{}"))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        jar.merge([
            CookieRecord(name: "greeting", value: "ਸਤ ਸ੍ਰੀ ਅਕਾਲ", domain: host, httpOnly: false),
            CookieRecord(name: "session", value: "s3cret", domain: host),
            CookieRecord(name: "theme", value: "a\u{7F}b", domain: host),
        ])
        #expect(!CookieJar.isSendable(CookieRecord(name: "greeting", value: "ਅਮਨ", domain: host)))
        #expect(!CookieJar.isSendable(CookieRecord(name: "a", value: "b; c=d", domain: host)))
        #expect(!CookieJar.isSendable(CookieRecord(name: "", value: "b", domain: host)))
        #expect(CookieJar.isSendable(CookieRecord(name: "session", value: "s3cret", domain: host)))

        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let read = FetchRequest(request: 8, url: "https://\(host)/orders")
        #expect(transport.urlRequest(for: read)?.value(forHTTPHeaderField: "Cookie") == "session=s3cret")
        let response = await fetched(transport, read)
        #expect(response.status == 200)
        #expect(response.error == nil)
        // The sign-out goes too.
        let signOut = FetchRequest(request: 9, method: "post", url: "https://\(host)/sign-out", body: .form(""))
        #expect(await fetched(transport, signOut).status == 200)
        let sent = StubOrigin.seen(host).map { $0.value(forHTTPHeaderField: "Cookie") }
        #expect(sent == ["session=s3cret", "session=s3cret"])
        // With only such a cookie, the request goes with none.
        let alone = CookieJar(origin: "https://\(host)", persistent: false)
        alone.merge([CookieRecord(name: "greeting", value: "ਅਮਨ", domain: host)])
        #expect(alone.header(for: URL(string: "https://\(host)/orders")!) == nil)
    }

    @Test func aStreamWithAHeaderValueOfAnotherScriptEndsAtOnce() async {
        let host = "streams-checked.shop.example"
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let state = await withCheckedContinuation { done in
            transport.open(stream: 2, url: "https://\(host)/broadcasting/stream", headers: [("x-navree-region", "ऑर्डर")]) {
                done.resume(returning: $0)
            }
        }
        #expect(state == .closed(status: nil))
        #expect(StubOrigin.seen(host).isEmpty)
    }
}
