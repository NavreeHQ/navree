import Foundation
import NavreeProtocol
import Testing

@testable import NavreeShell

@Test func anOriginIsOneHttpsOriginOrAPatternOfOneLevel() throws {
    let store = try #require(Origin("https://store.example.com"))
    #expect(store.contains(URL(string: "https://store.example.com/orders")!))
    #expect(store.contains(URL(string: "https://STORE.example.com:443/orders")!))
    #expect(!store.contains(URL(string: "http://store.example.com/orders")!))
    #expect(!store.contains(URL(string: "https://store.example.com:8443/orders")!))
    #expect(!store.contains(URL(string: "https://evil.com/store.example.com")!))
    #expect(!store.contains(URL(string: "https://user:pw@store.example.com/")!))

    let workspaces = try #require(Origin("https://*.example.com"))
    #expect(workspaces.contains(URL(string: "https://acme.example.com/")!))
    #expect(!workspaces.contains(URL(string: "https://example.com/")!))
    #expect(!workspaces.contains(URL(string: "https://a.b.example.com/")!))
    #expect(!workspaces.contains(URL(string: "https://acme.example.com.evil.com/")!))

    #expect(Origin("http://store.example.com") == nil)
    #expect(Origin("https://store.example.com/app") == nil)
    #expect(Origin("http://127.0.0.1:8095") != nil)
}

@Test func anythingButTheOriginLeavesForTheSystemOrIsRefused() throws {
    let origin = try #require(Origin("https://store.example.com"))
    #expect(origin.destination(of: "/orders/1?tab=items") == .app("/orders/1?tab=items"))
    #expect(origin.destination(of: "https://store.example.com/inventory") == .app("/inventory"))
    #expect(origin.destination(of: "https://pay.example/session/7") == .external(URL(string: "https://pay.example/session/7")!))
    #expect(origin.destination(of: "mailto:help@example.com") == .external(URL(string: "mailto:help@example.com")!))
    #expect(origin.destination(of: "javascript:alert(1)") == .refused)
    #expect(origin.destination(of: "file:///etc/passwd") == .refused)
    #expect(origin.destination(of: "data:text/html,hi") == .refused)
    #expect(origin.destination(of: "charkha://orders") == .refused)
}

@Test func aRouteMatchesBySegmentAndDecidesHowALinkIsPresented() {
    let rules = RouteRules([
        Navree.Route(match: "/orders/new", present: .sheet),
        Navree.Route(match: "/orders/*/edit", present: .sheet),
        Navree.Route(chrome: false, match: "/sign-in", present: .full),
        Navree.Route(match: "/orders/*/peek", present: .auto),
    ])
    #expect(rules.presentation(of: "/orders/new") == .sheet)
    #expect(rules.presentation(of: "/orders/42/edit?from=list") == .sheet)
    #expect(rules.presentation(of: "/orders/42") == .push)
    #expect(rules.presentation(of: "/orders/42/edit/more") == .push)
    #expect(rules.presentation(of: "/orders/new", asked: .replace) == .replace)
    // A link that leaves it to the app is a push the rules may change, and a
    // rule that leaves it to the app is a push.
    #expect(rules.presentation(of: "/orders/new", asked: .auto) == .sheet)
    #expect(rules.presentation(of: "/orders/42", asked: .auto) == .push)
    #expect(rules.presentation(of: "/orders/42/peek") == .push)
    #expect(rules.keepsChrome("/sign-in") == false)
    #expect(rules.keepsChrome("/orders") == true)
}

@Test func aDeepLinkOpensOnlyWhatTheShellNamesWithAGet() throws {
    let origin = try #require(Origin("https://store.example.com"))
    let links = DeepLinks(origin: origin, scheme: "charkha")
    let shell = try WireDecoder.decode(Navree.Shell.self, from: Data(#"""
        {"navree":1,"build":"b","name":"Store","home":"/","nav":{"groups":[{"items":[{"label":"Orders","href":"/orders"}]}]},
         "routes":[{"match":"/orders/*"}]}
        """#.utf8))
    #expect(links.address(for: URL(string: "https://store.example.com/orders/7?tab=items")!, shell: shell) == "/orders/7?tab=items")
    #expect(links.address(for: URL(string: "charkha://orders/7")!, shell: shell) == "/orders/7")
    #expect(links.address(for: URL(string: "https://store.example.com/orders")!, shell: shell) == "/orders")
    #expect(links.address(for: URL(string: "https://store.example.com/admin/users")!, shell: shell) == nil)
    #expect(links.address(for: URL(string: "https://store.example.com/orders/%2E%2E/admin")!, shell: shell) == nil)
    #expect(links.address(for: URL(string: "https://store.example.com/orders/a%5Cb")!, shell: shell) == nil)
    #expect(links.address(for: URL(string: "https://evil.example/orders/7")!, shell: shell) == nil)
    #expect(links.address(for: URL(string: "otherapp://orders/7")!, shell: shell) == nil)
    #expect(links.address(for: URL(string: "https://store.example.com/orders/7")!, shell: nil) == nil)
}

@Test func aFormIsEncodedAsABrowserEncodesIt() {
    let pairs = [
        Navree.Pair(name: "amount", value: "7,848.00"), Navree.Pair(name: "reason", value: "late delivery"),
        Navree.Pair(name: "filter[status][in]", value: "open"), Navree.Pair(name: "note", value: "₹ & ok"),
    ]
    #expect(
        FormEncoding.encode(pairs)
            == "amount=7%2C848.00&reason=late+delivery&filter%5Bstatus%5D%5Bin%5D=open&note=%E2%82%B9+%26+ok"
    )
    #expect(FormEncoding.encode([]) == "")
}

@Test func aCookieGoesOnlyWhereItsDomainPathAndSecurityAllow() {
    let now = Date(timeIntervalSince1970: 1_800_000_000)
    let session = CookieRecord(name: "session", value: "abc", domain: "store.example.com", path: "/")
    let expired = CookieRecord(name: "old", value: "x", domain: "store.example.com", expires: now.addingTimeInterval(-1))
    let scoped = CookieRecord(name: "admin", value: "y", domain: "store.example.com", path: "/admin")
    let wide = CookieRecord(name: "wide", value: "z", domain: ".example.com")
    let url = URL(string: "https://store.example.com/orders/1")!
    #expect(session.applies(to: url, at: now))
    #expect(!expired.applies(to: url, at: now))
    #expect(!scoped.applies(to: url, at: now))
    #expect(scoped.applies(to: URL(string: "https://store.example.com/admin/users")!, at: now))
    #expect(!scoped.applies(to: URL(string: "https://store.example.com/administer")!, at: now))
    #expect(wide.applies(to: url, at: now))
    #expect(!session.applies(to: URL(string: "http://store.example.com/orders")!, at: now))
}

@MainActor
@Test func theJarIsTheOneSourceOfTheSessionAndSpeaksOnlyToTheOrigin() throws {
    let origin = try #require(Origin("https://store.example.com"))
    let jar = CookieJar(origin: origin, persistent: false)
    jar.merge([CookieRecord(name: "session", value: "abc", domain: "store.example.com")])
    #expect(jar.header(for: URL(string: "https://store.example.com/orders")!) == "session=abc")
    #expect(jar.header(for: URL(string: "https://evil.example/orders")!) == nil)
    jar.merge([CookieRecord(name: "session", value: "def", domain: "store.example.com")])
    #expect(jar.header(for: URL(string: "https://store.example.com/")!) == "session=def")
}

@Test func aRedirectIsNeverFollowedOutOfTheOrigin() throws {
    let origin = try #require(Origin("https://store.example.com"))
    let page = URL(string: "https://store.example.com/orders/1/refund")!
    #expect(Transport.read(status: 303, location: "/orders/1", contentType: nil, data: Data(), from: page, origin: origin) == .redirect("/orders/1"))
    #expect(
        Transport.read(status: 302, location: "https://pay.example/7", contentType: nil, data: Data(), from: page, origin: origin)
            == .leave(URL(string: "https://pay.example/7")!)
    )
    if case .failed = Transport.read(status: 302, location: "javascript:x", contentType: nil, data: Data(), from: page, origin: origin) {
    } else {
        Issue.record("a redirect to a script was not refused")
    }
}

@MainActor
@Test func everyRequestCarriesTheNativeHeadersAndAPostItsContentType() throws {
    let origin = try #require(Origin("https://store.example.com"))
    let jar = CookieJar(origin: origin, persistent: false)
    let transport = Transport(
        base: origin.url!, origin: origin,
        descriptor: ClientDescriptor(platform: "ios", os: "26.5", form: "phone", app: "1.0", lib: "0.1.0"), jar: jar
    )
    let post = try #require(transport.urlRequest(for: OriginRequest(method: .post, address: "/orders/1/cancel")))
    #expect(post.value(forHTTPHeaderField: "X-Navree-Native") == "1")
    #expect(post.value(forHTTPHeaderField: "X-Navree-Client")?.hasPrefix("platform=ios; os=26.5; form=phone") == true)
    #expect(post.value(forHTTPHeaderField: "Accept") == "application/vnd.navree.native+json")
    #expect(post.value(forHTTPHeaderField: "Content-Type") == "application/x-www-form-urlencoded")
    #expect(post.httpMethod == "POST")
    let get = try #require(transport.urlRequest(for: OriginRequest(address: "/orders", form: [Navree.Pair(name: "search", value: "Kaur")])))
    #expect(get.url?.absoluteString == "https://store.example.com/orders?search=Kaur")
    #expect(transport.urlRequest(for: OriginRequest(address: "https://evil.example/")) == nil)
}
