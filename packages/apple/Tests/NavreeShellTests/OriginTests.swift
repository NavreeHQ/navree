import Foundation
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

/// One row of the table: whether `address` is ours for an app at `origin`.
/// The same shape as the `ours` list the runtime core keeps for every client,
/// so the rows can be read from that file as well.
struct AddressCase: Sendable, Decodable, CustomTestStringConvertible {
    var origin: String
    var also: [String]?
    var address: String
    var ours: Bool
    /// Plain `http` to this machine allowed, as a development build does.
    var loopback: Bool?

    var testDescription: String { "\(origin) \(address.debugDescription)" }
}

enum AddressTable {
    static let shop = "https://shop.example"
    static let local = "http://127.0.0.1:8095"

    static let ours: [AddressCase] = [
        // Whole addresses of the origin.
        AddressCase(origin: shop, address: "https://shop.example/", ours: true),
        AddressCase(origin: shop, address: "https://shop.example", ours: true),
        AddressCase(origin: shop, address: "HTTPS://SHOP.EXAMPLE/x", ours: true),
        AddressCase(origin: shop, address: "https://shop.example:443/x", ours: true),
        AddressCase(origin: shop, address: "https://shop.example/x?a=b://c", ours: true),
        AddressCase(origin: shop, address: "https://shop.example/a%20b", ours: true),
        AddressCase(
            origin: shop, address: "https://shop.example/%E0%A4%89%E0%A4%A4%E0%A5%8D%E0%A4%AA%E0%A4%BE%E0%A4%A6/1",
            ours: true
        ),
        AddressCase(origin: shop, address: "https://shop.example/x?filter[status]=won", ours: true),
        // Not whole, or another host's.
        AddressCase(origin: shop, address: "/x", ours: false),
        AddressCase(origin: shop, address: "//shop.example/x", ours: false),
        AddressCase(origin: shop, address: "//evil.example/x", ours: false),
        AddressCase(origin: shop, address: "//evil.example/x?a=b://c", ours: false),
        AddressCase(origin: shop, address: "/\\evil.example", ours: false),
        AddressCase(origin: shop, address: "https://shop.example@evil.example/", ours: false),
        AddressCase(origin: shop, address: "https://shop.example.evil.example/", ours: false),
        AddressCase(origin: shop, address: "https://evil.example/.shop.example", ours: false),
        AddressCase(origin: shop, address: "https://evil.example#@shop.example/", ours: false),
        AddressCase(origin: shop, address: "https://evil.example?@shop.example/", ours: false),
        AddressCase(origin: shop, address: "https://shop.example%2f@evil.example/", ours: false),
        AddressCase(origin: shop, address: "https://shop.example%2fevil.example/", ours: false),
        AddressCase(origin: shop, address: "https://shop.example\\@evil.example/", ours: false),
        AddressCase(origin: shop, address: "https://shop.example//evil.example/x", ours: false),
        AddressCase(origin: shop, address: "https://shop.example:8443/x", ours: false),
        AddressCase(origin: shop, address: "http://shop.example/", ours: false),
        AddressCase(origin: shop, address: "http://shop.example/x", ours: false),
        // White space and control characters, anywhere: refused, never trimmed.
        AddressCase(origin: shop, address: "\u{3000}//evil.example/x", ours: false),
        AddressCase(origin: shop, address: "\u{3000}https://shop.example/x", ours: false),
        AddressCase(origin: shop, address: " https://shop.example/x", ours: false),
        AddressCase(origin: shop, address: "https://shop.example/x ", ours: false),
        AddressCase(origin: shop, address: "https://shop.example/a b", ours: false),
        AddressCase(origin: shop, address: "https://shop.example/a\tb", ours: false),
        AddressCase(origin: shop, address: "https://shop.example/a\r\nb", ours: false),
        // Other schemes.
        AddressCase(origin: shop, address: "file:///Applications", ours: false),
        AddressCase(origin: shop, address: "file:///etc/hosts", ours: false),
        AddressCase(origin: shop, address: "javascript:1", ours: false),
        AddressCase(origin: shop, address: "data:text/html,x", ours: false),
        AddressCase(origin: shop, address: "intent://x", ours: false),
        AddressCase(origin: shop, address: "mailto:help@shop.example", ours: false),
        // A server on this machine, only when the build allows plain http.
        AddressCase(origin: local, address: "http://127.0.0.1:8095/orders", ours: true, loopback: true),
        AddressCase(origin: local, address: "https://127.0.0.1:8095/orders", ours: false, loopback: true),
        AddressCase(origin: local, address: "http://127.0.0.1:8096/orders", ours: false, loopback: true),
        AddressCase(origin: local, address: "http://127.0.0.1:8095/orders", ours: false),
    ]
}

struct OriginTests {
    @Test(arguments: AddressTable.ours)
    func anAddressIsOursOnlyWhenItIsWholeAndOnTheOrigin(_ row: AddressCase) {
        let site = Origin(row.origin, loopbackHTTP: row.loopback ?? false)
        #expect((site?.inside(row.address) != nil) == row.ours)
    }

    @Test func theAddressGivenBackIsWrittenFromThePartsRead() throws {
        let site = try #require(Origin("https://shop.example"))
        #expect(site.inside("HTTPS://SHOP.EXAMPLE/x")?.absoluteString == "https://shop.example/x")
        #expect(site.inside("https://shop.example/orders/7?tab=items#notes")?.absoluteString
            == "https://shop.example/orders/7?tab=items#notes")
        #expect(site.inside("https://shop.example")?.absoluteString == "https://shop.example")
    }

    @Test func anOriginIsOneHTTPSAddressOrAServerOnThisMachine() throws {
        let store = try #require(Origin("https://charkha.example/"))
        #expect(store.host == "charkha.example")
        #expect(store.text == "https://charkha.example")
        #expect(Origin("https://charkha.example/orders") == nil)
        #expect(Origin("https://charkha.example/?a=1") == nil)
        #expect(Origin("http://charkha.example", loopbackHTTP: true) == nil)
        #expect(Origin("http://127.0.0.1:8095") == nil)
        #expect(Origin("http://127.0.0.1:8095", loopbackHTTP: true)?.text == "http://127.0.0.1:8095")
        #expect(Origin("http://[::1]:8095", loopbackHTTP: true)?.host == "[::1]")
        #expect(Origin(" https://charkha.example") == nil)
        #expect(Origin("charkha.example") == nil)
    }

    @Test func aPictureIsAPathOfTheOriginOrAWholeAddressOfAHostTheAppNames() throws {
        let site = try #require(Origin("https://shop.example"))
        let hosts = ["images.shop.example"]
        #expect(site.picture("/storage/products/18.png", hosts: hosts)?.absoluteString
            == "https://shop.example/storage/products/18.png")
        #expect(site.picture("https://shop.example/a.png", hosts: hosts)?.absoluteString == "https://shop.example/a.png")
        #expect(site.picture("https://images.shop.example/a.png", hosts: hosts)?.absoluteString
            == "https://images.shop.example/a.png")
        #expect(site.picture("https://IMAGES.shop.example/a.png", hosts: hosts)?.host == "images.shop.example")
        for refused in [
            "//evil.example/a.png", "storage/a.png", "/\\evil.example/a.png", " /a.png", "/a b.png",
            "http://images.shop.example/a.png", "https://images.shop.example:8443/a.png", "https://cdn.evil.example/a.png",
            "https://user@images.shop.example/a.png", "file:///Applications/a.png", "",
        ] {
            #expect(site.picture(refused, hosts: hosts) == nil, "\(refused)")
        }
        // No other host by default.
        #expect(site.picture("https://images.shop.example/a.png", hosts: []) == nil)
        let local = try #require(Origin("http://127.0.0.1:8095", loopbackHTTP: true))
        #expect(local.picture("/a.png", hosts: [])?.absoluteString == "http://127.0.0.1:8095/a.png")
    }

    @Test func onlyAWebPageMailACallAMessageOrAPlaceIsOpened() {
        for opened in [
            "https://pay.example/invoice/7", "http://pay.example/", "mailto:help@shop.example", "tel:+919876543210",
            "sms:+919876543210", "geo:30.9010,75.8573",
        ] {
            #expect(SystemEffects.openable(opened) != nil, "\(opened)")
        }
        for refused in [
            "file:///Applications/Calculator.app", "FILE:///etc/hosts", "javascript:1", "shortcuts://run-shortcut",
            "x-apple-helpviewer://x", "ssh://host", "data:text/html,x", " https://pay.example/", "https://pay.example/a b",
            "https://user@pay.example/", "https:///x", "",
        ] {
            #expect(SystemEffects.openable(refused) == nil, "\(refused)")
        }
    }

    @MainActor
    @Test func aSharedAddressOfTheAppIsJoinedToTheOriginNeverReadAgainstIt() throws {
        let site = try #require(Origin("https://shop.example"))
        #expect(DeviceCalls.link(Navree.Share(href: "/orders/1"), site: site)?.absoluteString == "https://shop.example/orders/1")
        #expect(DeviceCalls.link(Navree.Share(href: "//evil.example/x"), site: site) == nil)
        #expect(DeviceCalls.link(Navree.Share(href: "orders/1"), site: site) == nil)
        #expect(DeviceCalls.link(Navree.Share(url: "https://pay.example/i/7"), site: site)?.absoluteString
            == "https://pay.example/i/7")
        #expect(DeviceCalls.link(Navree.Share(url: "file:///etc/hosts"), site: site) == nil)
    }
}

@MainActor
struct DoorTests {
    @Test func aFetchOfAnAddressThatIsNotTheOriginsIsAnsweredAsFailedAndNeverSent() async {
        let host = "doors.shop.example"
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        jar.merge([CookieRecord(name: "session", value: "s3cret", domain: host)])
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        for address in ["https://evil.example/x", "//evil.example/x", "https://\(host)//evil.example/x", "/orders"] {
            let response = await fetched(transport, FetchRequest(request: 3, url: address))
            #expect(response == .failed(3, .other), "\(address)")
        }
        #expect(StubOrigin.seen("evil.example").isEmpty)
    }

    @Test func aStreamFromAnAddressThatIsNotTheOriginsEndsAtOnce() async {
        let jar = CookieJar(origin: "https://streams.shop.example", persistent: false)
        let transport = HTTPTransport(jar: jar, configuration: StubOrigin.configuration)
        let state = await withCheckedContinuation { done in
            transport.open(stream: 1, url: "https://evil.example/broadcasting/stream", headers: []) { done.resume(returning: $0) }
        }
        #expect(state == .closed(status: nil))
        #expect(StubOrigin.seen("evil.example").isEmpty)
    }

    @Test func aPictureFromAnotherHostGoesWithNoCookieAndIsNotKept() async throws {
        let host = "doors-pictures.shop.example"
        let other = "images.doors.example"
        StubOrigin.answer(other, .init(
            status: 200, headers: ["Content-Type": "image/png", "Set-Cookie": "tracker=1; Path=/; Domain=\(other)"],
            body: "png"
        ))
        let jar = CookieJar(origin: "https://\(host)", persistent: false)
        jar.merge([CookieRecord(name: "wide", value: "s3cret", domain: ".doors.example")])
        let pictures = PictureLoader(jar: jar, hosts: [other], configuration: StubOrigin.configuration)
        let url = try #require(pictures.address("https://\(other)/a.png"))
        #expect(await pictures.load(url) == Data("png".utf8))
        #expect(StubOrigin.seen(other).last?.value(forHTTPHeaderField: "Cookie") == nil)
        #expect(!jar.records.contains { $0.name == "tracker" })
        // An address it may not ask for is never asked.
        #expect(await pictures.load(URL(string: "https://evil.example/a.png")!) == nil)
        #expect(StubOrigin.seen("evil.example").isEmpty)
    }
}
