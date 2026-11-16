import Foundation
import NavreeRuntime
import Testing

@testable import NavreeShell

struct WebRulesTests {
    static let site = Origin("https://shop.example")!

    @Test func onlyHostsOfTwoLabelsOrMoreCanBeNamedForTheWebView() {
        #expect(
            WebRules.hosts(["checkout.razorpay.com", "*.Stripe.com", "*.com", "com", "", "a..b", "-a.example", "x.example/",
                            "evil.example:443", "*.*.example"])
                == ["checkout.razorpay.com", "*.stripe.com"]
        )
    }

    @Test func aPatternCoversOneLabelOrMoreUnderItsNameAndNothingBeside() {
        let hosts = ["*.stripe.com", "checkout.razorpay.com"]
        #expect(WebRules.covers(hosts, "js.stripe.com"))
        #expect(WebRules.covers(hosts, "a.b.stripe.com"))
        #expect(!WebRules.covers(hosts, "stripe.com"))
        #expect(!WebRules.covers(hosts, "evilstripe.com"))
        #expect(!WebRules.covers(hosts, "stripe.com.evil.example"))
        #expect(WebRules.covers(hosts, "CHECKOUT.razorpay.com"))
        #expect(!WebRules.covers(hosts, "api.razorpay.com"))
    }

    @Test func aFrameIsTheOriginsAnEmptyOneOrOfAHostTheAppNamesForTheWebView() throws {
        let hosts = ["*.stripe.com"]
        for allowed in ["https://shop.example/embed", "about:blank", "about:srcdoc", "https://js.stripe.com/v3/frame"] {
            #expect(WebRules.allowsFrame(try #require(URL(string: allowed)), site: Self.site, hosts: hosts), "\(allowed)")
        }
        for refused in [
            "https://evil.example/", "http://js.stripe.com/", "https://js.stripe.com:8443/", "https://stripe.com/",
            "data:text/html,x", "https://shop.example//evil.example/x",
        ] {
            #expect(!WebRules.allowsFrame(try #require(URL(string: refused)), site: Self.site, hosts: hosts), "\(refused)")
        }
        // None by default.
        #expect(!WebRules.allowsFrame(try #require(URL(string: "https://js.stripe.com/")), site: Self.site, hosts: []))
    }

    @Test func theContentRulesBlockEveryLoadThenLetTheOriginsAndTheNamedHostsThrough() throws {
        let text = WebRules.contentRules(site: Self.site, hosts: ["*.stripe.com", "checkout.razorpay.com"])
        let rules = try #require(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [[String: [String: String]]])
        #expect(rules.first?["trigger"]?["url-filter"] == ".*")
        #expect(rules.first?["action"]?["type"] == "block")
        let passed = rules.dropFirst().compactMap { $0["trigger"]?["url-filter"] }
        #expect(passed == [
            "^https://shop\\.example/", "^data:", "^blob:", "^about:", "^https://([a-z0-9-]+\\.)+stripe\\.com/",
            "^https://checkout\\.razorpay\\.com/",
        ])
        #expect(rules.dropFirst().allSatisfy { $0["action"]?["type"] == "ignore-previous-rules" })
        // A server on this machine keeps its port.
        let local = try #require(Origin("http://127.0.0.1:8095", loopbackHTTP: true))
        #expect(WebRules.contentRules(site: local, hosts: []).contains(#"^http://127\\.0\\.0\\.1:8095/"#))
    }

    @Test func theDocumentScriptNamesTheOriginAsABrowserWritesIt() throws {
        let explicit = try #require(Origin("https://shop.example:443"))
        #expect(explicit.serialized == "https://shop.example")
        let script = WebRules.documentScript(site: explicit, hosts: ["*.stripe.com"])
        #expect(script.contains(#"var origins = ["https://shop.example"];"#))
        #expect(script.contains("*.stripe.com"))
        #expect(script.contains("window.stop()"))
    }
}
