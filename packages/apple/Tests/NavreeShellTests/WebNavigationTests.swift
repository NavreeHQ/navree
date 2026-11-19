import Foundation
import JavaScriptCore
import NavreeRuntime
import Testing

@testable import NavreeShell

/// What becomes of a navigation in the web view: the core hears of a tap only
/// when the system says the person activated a link and the web view saw real
/// person input.
struct WebNavigationTests {
    @Test func onlyALinkThePersonActivatedIsToldToTheCore() {
        // A tapped link, to the app's own address or to anywhere else.
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: true) == .tap)
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: false) == .tap)
        // A tapped link to the address the page was loaded from is a tap too.
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: true, loaded: true) == .tap)
    }

    @Test func whatAPageDoesByItselfNeverLeavesTheOriginAndIsToldToNobody() {
        // `location = "tel:+919814012345"`, `sms:`, `mailto:`, another site;
        // a redirect the server answers; a form posted elsewhere: none of
        // them is the app's own address, and none is a link activated.
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: true, ours: false) == .cancel)
        // On the origin the same stays in the web view: a form posted, a
        // redirect of the page's own, its reload.
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: true, ours: true) == .allow)
        // The page the app asked for loads.
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: true, ours: true, loaded: true) == .allow)
    }

    @Test func aFrameStaysTheOriginsOrEmptyAndNeverTakesThePersonAnywhere() {
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: false, ours: true) == .allow)
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: false, ours: false, empty: true) == .allow)
        #expect(WebNavigationRules.decide(linkActivated: false, mainFrame: false, ours: false) == .cancel)
        #expect(WebNavigationRules.decide(linkActivated: true, mainFrame: false, ours: false) == .cancel)
    }

    @Test func aNewWindowIsALinkOnlyWhenThePersonActivatedOne() {
        #expect(WebNavigationRules.newWindow(linkActivated: true) == .tap)
        // `window.open("sms:+919814012345")` from a script.
        #expect(WebNavigationRules.newWindow(linkActivated: false) == .cancel)
    }

    @Test func aScriptsOwnClickIsNotTakenForATap() throws {
        // 1. Without recent person input, a navigation WebKit tags `.linkActivated`
        // (such as `anchor.click()`) is never told to the core as a tap: outside
        // the origin it is cancelled; on the origin it stays inside the web view.
        #expect(
            WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: false, userInteracted: false)
                == .cancel
        )
        #expect(
            WebNavigationRules.decide(linkActivated: true, mainFrame: true, ours: true, userInteracted: false)
                == .allow
        )
        #expect(
            WebNavigationRules.decide(
                linkActivated: true, mainFrame: true, ours: true, loaded: true, userInteracted: false
            ) == .allow
        )
        #expect(WebNavigationRules.newWindow(linkActivated: true, userInteracted: false) == .cancel)

        // 2. The interaction window opens only after real input and expires cleanly.
        var tracker = InteractionTracker()
        let start = ContinuousClock.now
        #expect(!tracker.hasRecent(at: start))
        tracker.note(at: start)
        #expect(tracker.hasRecent(at: start.advanced(by: .milliseconds(200))))
        #expect(!tracker.hasRecent(at: start.advanced(by: .seconds(2))))

        // 3. Both document-start scripts block synthetic clicks (`anchor.click()`,
        // `span.click()` inside a detached `<a>`, SVG `<a xlink:href>`, and
        // `dispatchEvent` with `isTrusted == false`), while keeping real trusted clicks intact.
        let site = try #require(Origin("https://shop.example"))
        let ctx = try #require(JSContext())
        ctx.evaluateScript(Self.clickHarness)
        #expect(ctx.exception == nil)
        ctx.evaluateScript(WebRules.documentScript(site: site, hosts: []))
        #expect(ctx.exception == nil)
        ctx.evaluateScript(WebRules.pageClickScript(site: site))
        #expect(ctx.exception == nil)

        // Detached `<a href="tel:+919814012345">`, nested `<span>.click()`, and SVG `<a xlink:href>`:
        // none invokes WebKit's native link click or navigates anywhere.
        ctx.evaluateScript("""
            var telLink = new Anchor('tel:+919814012345');
            telLink.click();

            var nestedTel = new Anchor('tel:+919814012345');
            var childSpan = new Span(nestedTel);
            childSpan.click();

            var svgTel = new SvgAnchor('tel:+919814012345');
            svgTel.click();
            """)
        #expect(ctx.evaluateScript("nativeClicks")?.toInt32() == 0)
        #expect(ctx.evaluateScript("assignedURLs.length")?.toInt32() == 0)

        // Detached same-origin `<a href="/orders/2">` and nested `<span>.click()` inside `<a href="/orders/3">`
        // navigate via `location.assign` (`.other`, never `.linkActivated`) without native click.
        ctx.evaluateScript("""
            var internalLink = new Anchor('/orders/2');
            internalLink.click();

            var nestedInternal = new Anchor('/orders/3');
            var internalSpan = new Span(nestedInternal);
            internalSpan.click();
            """)
        #expect(ctx.evaluateScript("nativeClicks")?.toInt32() == 0)
        #expect(ctx.evaluateScript("assignedURLs[0]")?.toString() == "https://shop.example/orders/2")
        #expect(ctx.evaluateScript("assignedURLs[1]")?.toString() == "https://shop.example/orders/3")

        // Untrusted `dispatchEvent` on an attached link is cancelled (`defaultPrevented`),
        // whereas a real person tap (`isTrusted: true`) is not.
        ctx.evaluateScript("""
            var attached = new Anchor('tel:+919814012345');
            var fakeClick = { type: 'click', target: attached, isTrusted: false, bubbles: true,
              defaultPrevented: false, preventDefault: function () { this.defaultPrevented = true; } };
            attached.dispatchEvent(fakeClick);
            dispatchWindowClick(fakeClick);

            var realClick = { type: 'click', target: attached, isTrusted: true, bubbles: true,
              defaultPrevented: false, preventDefault: function () { this.defaultPrevented = true; } };
            dispatchWindowClick(realClick);
            """)
        #expect(ctx.evaluateScript("fakeClick.defaultPrevented")?.toBool() == true)
        #expect(ctx.evaluateScript("realClick.defaultPrevented")?.toBool() == false)
    }

    static let clickHarness = """
        var nativeClicks = 0;
        var assignedURLs = [];
        var windowListeners = [];
        function EventTarget() {}
        EventTarget.prototype.dispatchEvent = function (ev) { return !ev.defaultPrevented; };
        function HTMLElement() {}
        HTMLElement.prototype = Object.create(EventTarget.prototype);
        HTMLElement.prototype.click = function () { nativeClicks += 1; };
        function SVGElement() {}
        SVGElement.prototype = Object.create(EventTarget.prototype);
        SVGElement.prototype.click = function () { nativeClicks += 1; };
        function Anchor(href) {
          this.tagName = 'A';
          this.href = href;
          this.target = '';
          this.parentNode = null;
        }
        Anchor.prototype = Object.create(HTMLElement.prototype);
        Anchor.prototype.hasAttribute = function (n) { return n === 'href' && Boolean(this.href); };
        Anchor.prototype.getAttribute = function (n) { return n === 'href' ? this.href : (n === 'target' ? this.target : null); };
        function Span(parent) {
          this.tagName = 'SPAN';
          this.parentNode = parent || null;
        }
        Span.prototype = Object.create(HTMLElement.prototype);
        function SvgAnchor(xlinkHref) {
          this.tagName = 'a';
          this.href = { baseVal: xlinkHref };
          this.parentNode = null;
          this._xlink = xlinkHref;
        }
        SvgAnchor.prototype = Object.create(SVGElement.prototype);
        SvgAnchor.prototype.hasAttribute = function (n) { return n === 'xlink:href' && Boolean(this._xlink); };
        SvgAnchor.prototype.getAttribute = function (n) { return n === 'xlink:href' ? this._xlink : null; };
        function URL(raw, base) {
          if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(raw)) {
            var m = /^(https?:\\/\\/[^\\/]+)(\\/.*)?$/i.exec(raw);
            if (m) { this.origin = m[1].toLowerCase(); this.href = raw; }
            else { this.origin = 'null'; this.href = raw; }
          } else {
            var bm = /^(https?:\\/\\/[^\\/]+)/i.exec(base || '');
            this.origin = bm ? bm[1].toLowerCase() : 'null';
            this.href = this.origin + (raw.charAt(0) === '/' ? raw : '/' + raw);
          }
        }
        var location = {
          origin: 'https://shop.example',
          href: 'https://shop.example/orders',
          assign: function (u) { assignedURLs.push(u); }
        };
        var document = {
          documentElement: { querySelectorAll: function () { return []; } },
          baseURI: 'https://shop.example/orders'
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
        function dispatchWindowClick(evt) {
          for (var i = 0; i < windowListeners.length; i++) {
            var l = windowListeners[i];
            if (l.type === 'click' && l.capture) l.fn(evt);
          }
          for (var j = 0; j < windowListeners.length; j++) {
            var b = windowListeners[j];
            if (b.type === 'click' && !b.capture && evt.bubbles) b.fn(evt);
          }
        }
        """
}
