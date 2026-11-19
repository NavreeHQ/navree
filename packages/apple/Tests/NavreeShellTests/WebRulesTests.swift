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
        let cleanNote = folder.appendingPathComponent("invoice.txt")
        try Data("Invoice 20481, Ludhiana".utf8).write(to: cleanNote)

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
                frameURL: URL(string: "https://shop.example/"),
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

        // Directory uploads, foreign origins, opaque sandboxed origins (`nil`) and `about:` frames open no picker.
        let disallowedCases: [(dirs: Bool, url: URL?, main: Bool)] = [
            (dirs: true, url: URL(string: "https://shop.example/"), main: true),
            (dirs: false, url: URL(string: "https://evil.example/frame"), main: false),
            (dirs: false, url: nil, main: false),
            (dirs: false, url: URL(string: "about:srcdoc"), main: false),
        ]
        for disallowed in disallowedCases {
            let blocked = await withCheckedContinuation { (continuation: CheckedContinuation<[URL]?, Never>) in
                coordinator.handleOpenPanel(
                    multiple: false, allowsDirectories: disallowed.dirs,
                    frameURL: disallowed.url, mainFrame: disallowed.main
                ) { continuation.resume(returning: $0) }
            }
            #expect(blocked == nil)
        }
        #expect(picker.calls.count == 1)

        // Re-entering `handleOpenPanel` while an earlier pick is still preparing off the main actor
        // completes the superseded handler with `nil` without prematurely completing the new one.
        let (enteredStream, enteredContinuation) = AsyncStream<Void>.makeStream()
        let (releaseStream, releaseContinuation) = AsyncStream<Void>.makeStream()
        shell.effects.keeper.files.workObserver = {
            enteredContinuation.yield()
            enteredContinuation.finish()
            for await _ in releaseStream { break }
        }
        picker.urls = [geotaggedPhoto]
        var firstOutcomes: [[URL]?] = []
        coordinator.handleOpenPanel(
            multiple: false, allowsDirectories: false,
            frameURL: URL(string: "https://shop.example/"), mainFrame: true
        ) { firstOutcomes.append($0) }

        for await _ in enteredStream { break }
        shell.effects.keeper.files.workObserver = nil
        picker.urls = [cleanNote]

        let secondTask = Task { @MainActor in
            await withCheckedContinuation { (continuation: CheckedContinuation<[URL]?, Never>) in
                coordinator.handleOpenPanel(
                    multiple: false, allowsDirectories: false,
                    frameURL: URL(string: "https://shop.example/"), mainFrame: true
                ) { continuation.resume(returning: $0) }
            }
        }
        releaseContinuation.finish()
        let secondChosen = await secondTask.value
        #expect(firstOutcomes.count == 1)
        if let recorded = firstOutcomes.first {
            #expect(recorded == nil)
        }
        #expect(secondChosen?.map(\.lastPathComponent) == ["invoice.txt"])

        // File drops anywhere on the page (such as a `<div class="dropzone">`) and file pastes
        // are blocked by `documentScript`, while plain-text paste is untouched.
        let ctx = try #require(JSContext())
        ctx.evaluateScript(Self.domHarness)
        ctx.evaluateScript(WebRules.documentScript(site: Self.site, hosts: [], answersFilePicker: true))
        #expect(ctx.exception == nil)
        ctx.evaluateScript("""
            var dropzone = makeElement('DIV', {});
            formNode.appendChild(dropzone);
            var dropEvt = makeEvent('drop', dropzone, true, true);
            dropEvt.dataTransfer = { files: [{ name: 'amritsar.jpg' }], types: ['Files'] };
            dispatchOnWindow(dropEvt);

            var filePasteEvt = makeEvent('paste', dropzone, true, true);
            filePasteEvt.clipboardData = { files: [{ name: 'amritsar.jpg' }], items: [{ kind: 'file' }] };
            dispatchOnWindow(filePasteEvt);

            var textPasteEvt = makeEvent('paste', textInput, true, true);
            textPasteEvt.clipboardData = { files: [], items: [{ kind: 'string' }], types: ['text/plain'] };
            dispatchOnWindow(textPasteEvt);
            """)
        #expect(ctx.evaluateScript("dropEvt.defaultPrevented")?.toBool() == true)
        #expect(ctx.evaluateScript("filePasteEvt.defaultPrevented")?.toBool() == true)
        #expect(ctx.evaluateScript("textPasteEvt.defaultPrevented")?.toBool() == false)

        // Ending the session wipes both the prepared web files and the refusal words.
        shell.effects.run(.forgetSession, for: shell.client)
        #expect(shell.effects.fileNotes.webWords(screen: screenID).isEmpty)
        #expect(!FileManager.default.fileExists(atPath: urls[0].path))
    }

    @Test func whereTheSystemGivesNoHookAWebPagesFileFieldIsSwitchedOffAndSaysSo() throws {
        let ctx = try #require(JSContext())
        ctx.evaluateScript(Self.domHarness)
        #expect(ctx.exception == nil)

        ctx.evaluateScript(WebRules.documentScript(site: Self.site, hosts: [], answersFilePicker: false))
        #expect(ctx.exception == nil)
        ctx.evaluateScript(WebRules.pageClickScript(site: Self.site, answersFilePicker: false))
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

        // Even if a script clears `disabled` and clicks the input, calls `showPicker()`,
        // or calls `.click()` on a detached file input, the guards block it.
        ctx.evaluateScript("""
            receiptInput.disabled = false;
            var clickEvt = makeEvent('click', receiptInput, true, true);
            dispatchOnWindow(clickEvt);

            receiptInput.disabled = false;
            receiptInput.showPicker();

            var detachedInput = makeElement('INPUT', { type: 'file' });
            detachedInput.click();
            """)
        #expect(ctx.evaluateScript("receiptInput.disabled")?.toBool() == true)
        #expect(ctx.evaluateScript("clickEvt.defaultPrevented")?.toBool() == true)
        #expect(ctx.evaluateScript("nativeShowPickers")?.toInt32() == 0)
        #expect(ctx.evaluateScript("detachedInput.disabled")?.toBool() == true)
        #expect(ctx.evaluateScript("nativeElementClicks")?.toInt32() == 0)
    }

    static let domHarness = """
        var windowListeners = [];
        var mutationCallbacks = [];
        var nativeElementClicks = 0;
        var nativeShowPickers = 0;
        function EventTarget() {}
        EventTarget.prototype.dispatchEvent = function (ev) { return !ev.defaultPrevented; };
        function HTMLElement() {}
        HTMLElement.prototype = Object.create(EventTarget.prototype);
        HTMLElement.prototype.click = function () { nativeElementClicks += 1; };
        function HTMLInputElement() {}
        HTMLInputElement.prototype = Object.create(HTMLElement.prototype);
        HTMLInputElement.prototype.showPicker = function () { nativeShowPickers += 1; };
        function makeElement(tag, attrs) {
          var upper = tag.toUpperCase();
          var proto = upper === 'INPUT' ? HTMLInputElement.prototype : HTMLElement.prototype;
          var map = {};
          if (attrs) { for (var k in attrs) map[k] = String(attrs[k]); }
          var el = Object.create(proto);
          el.tagName = upper;
          el.type = map.type || '';
          el.disabled = false;
          el.textContent = '';
          el.parentNode = null;
          el.children = [];
          Object.defineProperty(el, 'nextSibling', {
            get: function () {
              if (!this.parentNode) return null;
              var idx = this.parentNode.children.indexOf(this);
              return (idx >= 0 && idx + 1 < this.parentNode.children.length) ? this.parentNode.children[idx + 1] : null;
            }
          });
          el.getAttribute = function (n) { return Object.prototype.hasOwnProperty.call(map, n) ? map[n] : null; };
          el.setAttribute = function (n, v) { map[n] = String(v); if (n === 'type') this.type = String(v); };
          el.hasAttribute = function (n) { return Object.prototype.hasOwnProperty.call(map, n); };
          el.appendChild = function (child) {
            child.parentNode = this;
            this.children.push(child);
            return child;
          };
          el.insertBefore = function (child, ref) {
            child.parentNode = this;
            if (!ref) { this.children.push(child); return child; }
            var idx = this.children.indexOf(ref);
            if (idx < 0) this.children.push(child);
            else this.children.splice(idx, 0, child);
            return child;
          };
          el.querySelectorAll = function () {
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
