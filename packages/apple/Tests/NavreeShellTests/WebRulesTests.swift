import Foundation
import ImageIO
import JavaScriptCore
import NavreeProtocol
import NavreeRuntime
import Testing

@testable import NavreeShell

@MainActor
private final class PlayedWebFilePicker: WebFilePicker {
    var urls: [URL]?
    private(set) var calls: [Bool] = []

    init(_ urls: [URL]?) {
        self.urls = urls
    }

    func pick(multiple: Bool) async -> [URL]? {
        calls.append(multiple)
        return urls
    }
}

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

    @MainActor
    @Test func aPageInTheWebViewPicksFilesThroughTheSameCheckAsANativeField() async throws {
        #expect(WebFileRules.systemAnswersOpenPanel)

        let shell = try Shell()
        shell.client.begin()
        let screenID = try #require(shell.client.nav.front)

        let folder = try PhotoPlaceTests.folder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let geotaggedPhoto = folder.appendingPathComponent("amritsar.jpg")
        try PhotoPlaceTests.picture(properties: [kCGImagePropertyGPSDictionary: PhotoPlaceTests.place])
            .write(to: geotaggedPhoto)
        let geotaggedFilm = try await PhotoPlaceTests.film(in: folder, named: "hall-bazaar.mov", withPlace: true)
        let designFile = folder.appendingPathComponent("banner.psd")
        try Data([0x38, 0x42, 0x50, 0x53, 0x00, 0x01] + [UInt8](repeating: 0, count: 20)).write(to: designFile)

        let config = ShellConfiguration(origin: "https://shop.example", appID: "com.navree.demo", appVersion: "1.0")
        let page = WebPageView(
            url: try #require(URL(string: "https://shop.example/checkout")),
            serial: 1, screen: screenID, client: shell.client, configuration: config,
            failed: {}, titled: { _ in }
        )
        let coordinator = page.makeCoordinator()
        coordinator.made = (shell.effects.keeper.number, shell.effects.jar?.session ?? 0)
        let picker = PlayedWebFilePicker([geotaggedPhoto, geotaggedFilm, designFile])
        coordinator.filePicker = picker

        let chosen = await withCheckedContinuation { (continuation: CheckedContinuation<[URL]?, Never>) in
            coordinator.handleOpenPanel(
                multiple: true,
                allowsDirectories: false,
                frameURL: URL(string: "https://shop.example/checkout"),
                mainFrame: true
            ) { urls in
                continuation.resume(returning: urls)
            }
        }

        #expect(picker.calls == [true])
        let urls = try #require(chosen)
        #expect(urls.map(\.lastPathComponent) == ["amritsar.jpg", "hall-bazaar.mov"])

        let strippedPhoto = try Data(contentsOf: urls[0])
        #expect(PhotoPlace.isBare(strippedPhoto))
        #expect(await FilmPlace.isBare(urls[1]))

        // The refused design file is named with the same words as on a native field.
        #expect(shell.effects.fileNotes.webWords(screen: screenID) == [
            Words.refused("banner.psd", .placeStays)
        ])

        // Directory uploads and frames outside allowed origins are refused without opening the picker.
        let dirResult = await withCheckedContinuation { (continuation: CheckedContinuation<[URL]?, Never>) in
            coordinator.handleOpenPanel(
                multiple: false, allowsDirectories: true,
                frameURL: URL(string: "https://shop.example/checkout"), mainFrame: true
            ) { continuation.resume(returning: $0) }
        }
        #expect(dirResult == nil)

        let foreignResult = await withCheckedContinuation { (continuation: CheckedContinuation<[URL]?, Never>) in
            coordinator.handleOpenPanel(
                multiple: false, allowsDirectories: false,
                frameURL: URL(string: "https://evil.example/frame"), mainFrame: false
            ) { continuation.resume(returning: $0) }
        }
        #expect(foreignResult == nil)
        #expect(picker.calls.count == 1)

        // Ending the session wipes both the prepared web files and the refusal words.
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(shell.effects.fileNotes.webWords(screen: screenID).isEmpty)
        #expect(!FileManager.default.fileExists(atPath: urls[0].path))
    }

    @Test func whereTheSystemGivesNoHookAWebPagesFileFieldIsSwitchedOffAndSaysSo() throws {
        let ctx = try #require(JSContext())
        ctx.evaluateScript(Self.domHarness)
        #expect(ctx.exception == nil)

        let script = WebRules.documentScript(site: Self.site, hosts: [], answersFilePicker: false)
        ctx.evaluateScript(script)
        #expect(ctx.exception == nil)

        let expectedWords = WebFileRules.disabledWords
        #expect(ctx.evaluateScript("receiptInput.disabled")?.toBool() == true)
        #expect(ctx.evaluateScript("receiptInput.getAttribute('aria-disabled')")?.toString() == "true")
        #expect(ctx.evaluateScript("textInput.disabled")?.toBool() == false)
        #expect(ctx.evaluateScript("formNode.children.length")?.toInt32() == 3)
        #expect(ctx.evaluateScript("formNode.children[1].getAttribute('data-navree-file-note')")?.toString() == "true")
        #expect(ctx.evaluateScript("formNode.children[1].textContent")?.toString() == expectedWords)

        // Dynamically added file fields are switched off and annotated once by the MutationObserver.
        ctx.evaluateScript("""
            var dynamicInput = makeElement('INPUT', { type: 'file' });
            formNode.appendChild(dynamicInput);
            notifyMutations([{ target: formNode, addedNodes: [dynamicInput] }]);
            notifyMutations([{ target: receiptInput, addedNodes: [] }]);
            """)
        #expect(ctx.exception == nil)
        #expect(ctx.evaluateScript("dynamicInput.disabled")?.toBool() == true)
        #expect(ctx.evaluateScript("formNode.children.length")?.toInt32() == 5)
        #expect(ctx.evaluateScript("formNode.children[4].textContent")?.toString() == expectedWords)

        // Even if a script clears `disabled` and clicks the input, the capture listener stops it.
        ctx.evaluateScript("""
            receiptInput.disabled = false;
            var clickEvt = makeEvent('click', receiptInput, true, true);
            dispatchOnWindow(clickEvt);
            """)
        #expect(ctx.evaluateScript("receiptInput.disabled")?.toBool() == true)
        #expect(ctx.evaluateScript("clickEvt.defaultPrevented")?.toBool() == true)
    }

    static let domHarness = """
        var windowListeners = [];
        var mutationCallbacks = [];
        function makeElement(tag, attrs) {
          var map = {};
          if (attrs) { for (var k in attrs) map[k] = String(attrs[k]); }
          var el = {
            tagName: tag.toUpperCase(),
            type: map.type || '',
            disabled: false,
            textContent: '',
            parentNode: null,
            children: [],
            get nextSibling() {
              if (!this.parentNode) return null;
              var idx = this.parentNode.children.indexOf(this);
              return (idx >= 0 && idx + 1 < this.parentNode.children.length) ? this.parentNode.children[idx + 1] : null;
            },
            getAttribute: function (n) { return Object.prototype.hasOwnProperty.call(map, n) ? map[n] : null; },
            setAttribute: function (n, v) { map[n] = String(v); if (n === 'type') this.type = String(v); },
            hasAttribute: function (n) { return Object.prototype.hasOwnProperty.call(map, n); },
            appendChild: function (child) {
              child.parentNode = this;
              this.children.push(child);
              return child;
            },
            insertBefore: function (child, ref) {
              child.parentNode = this;
              if (!ref) { this.children.push(child); return child; }
              var idx = this.children.indexOf(ref);
              if (idx < 0) this.children.push(child);
              else this.children.splice(idx, 0, child);
              return child;
            },
            querySelectorAll: function () {
              var out = [];
              function walk(node) {
                for (var i = 0; i < node.children.length; i++) {
                  var c = node.children[i];
                  if (c.tagName === 'INPUT') out.push(c);
                  walk(c);
                }
              }
              walk(this);
              return out;
            }
          };
          return el;
        }
        var rootNode = makeElement('HTML', {});
        var formNode = makeElement('FORM', {});
        var receiptInput = makeElement('INPUT', { type: 'file' });
        var textInput = makeElement('INPUT', { type: 'text' });
        formNode.appendChild(receiptInput);
        formNode.appendChild(textInput);
        rootNode.appendChild(formNode);
        var location = { origin: 'https://shop.example', href: 'https://shop.example/upload' };
        var document = {
          documentElement: rootNode,
          baseURI: 'https://shop.example/upload',
          createElement: function (tag) { return makeElement(tag, {}); }
        };
        var window = {
          location: location,
          document: document,
          stop: function () {},
          addEventListener: function (type, fn, capture) {
            windowListeners.push({ type: type, fn: fn, capture: Boolean(capture) });
          }
        };
        window.top = window;
        function MutationObserver(cb) { mutationCallbacks.push(cb); }
        MutationObserver.prototype.observe = function () {};
        function notifyMutations(records) {
          for (var i = 0; i < mutationCallbacks.length; i++) mutationCallbacks[i](records);
        }
        function makeEvent(type, target, trusted, bubbles) {
          return {
            type: type,
            target: target,
            isTrusted: trusted,
            bubbles: bubbles,
            defaultPrevented: false,
            stopped: false,
            preventDefault: function () { this.defaultPrevented = true; },
            stopImmediatePropagation: function () { this.stopped = true; }
          };
        }
        function dispatchOnWindow(evt) {
          for (var i = 0; i < windowListeners.length; i++) {
            if (evt.stopped) return;
            var l = windowListeners[i];
            if (l.type === evt.type && l.capture) l.fn(evt);
          }
          for (var j = 0; j < windowListeners.length; j++) {
            if (evt.stopped) return;
            var b = windowListeners[j];
            if (b.type === evt.type && !b.capture && evt.bubbles) b.fn(evt);
          }
        }
        """
}
