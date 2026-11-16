import Foundation
import NavreeProtocol
import NavreeRuntime
import SwiftUI
import WebKit

/// A page shown whole in the system web view, inside the app's own bars, at
/// the address the runtime core gives, loaded again whenever its serial moves.
///
/// What a page in it can do is what a browser tab on the same site can do, and
/// nothing more: there is no script bridge to the app, no pop-up, no file. A
/// link the person taps is the core's to follow (a native screen, the system
/// browser, or nothing): the web view's own navigation is cancelled. What a
/// page does by itself (its script, a redirect) stays on the origin or goes
/// nowhere, and is never told to the core as a tap (``WebNavigationRules``).
struct WebPageView {
    let url: URL
    let serial: Int
    let screen: ScreenID
    let client: NativeClient
    let configuration: ShellConfiguration
    /// The page could not be shown: it never arrived, it was stopped before
    /// it was, or its process ended twice. The screen shows it failed.
    let failed: @MainActor () -> Void
    /// The title the page gives itself.
    let titled: @MainActor (String) -> Void

    @MainActor
    func makeWebView(_ coordinator: Coordinator) -> WKWebView {
        let settings = WKWebViewConfiguration()
        // The session's own store, never the app's default one: a page of the
        // next person finds nothing a page of the last one left. The web
        // view is held to the session it was made in.
        let effects = client.effects as? SystemEffects
        settings.websiteDataStore = effects?.keeper.webStore() ?? .nonPersistent()
        coordinator.made = (effects?.keeper.number ?? 0, effects?.jar?.session ?? 0)
        settings.applicationNameForUserAgent = Wire.userAgentToken(platform: Self.platform, lib: Library.version)
        settings.preferences.javaScriptCanOpenWindowsAutomatically = false
        settings.defaultWebpagePreferences.allowsContentJavaScript = true
        // No `WKScriptMessageHandler` is ever added to the user content
        // controller: the page has no way to reach the app.
        let view = WKWebView(frame: .zero, configuration: settings)
        view.navigationDelegate = coordinator
        view.uiDelegate = coordinator
        view.allowsBackForwardNavigationGestures = false
        #if DEBUG
        view.isInspectable = true
        #else
        view.isInspectable = false
        #endif
        coordinator.observe(view)
        coordinator.load(url, serial: serial, in: view)
        return view
    }

    static var platform: String {
        #if os(macOS)
        "macos"
        #else
        "ios"
        #endif
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var parent: WebPageView
        private var title: NSKeyValueObservation?
        private(set) var serial: Int?
        private var loaded: URL?
        /// Whether a page of this load has been shown.
        private var shown = false
        /// How often the page's process has ended since its serial moved.
        private var ended = 0
        /// The session the web view was made in, by the session's number and
        /// the jar's: its store is that session's, and once the session has
        /// ended it loads nothing more and gives the jar nothing.
        var made = (session: 0, cookies: 0)

        /// Whether the session the web view was made in is still the session.
        private var isCurrent: Bool {
            guard let effects = parent.client.effects as? SystemEffects else { return true }
            return effects.keeper.number == made.session
        }

        init(_ parent: WebPageView) {
            self.parent = parent
        }

        func observe(_ view: WKWebView) {
            title = view.observe(\.title, options: [.new]) { [weak self] view, _ in
                MainActor.assumeIsolated {
                    // Only a page of the origin names the bar.
                    guard let self, let title = view.title, !title.isEmpty, let url = view.url, self.isThePagesOwn(url)
                    else { return }
                    self.parent.titled(title)
                }
            }
        }

        /// Makes the store hold the session's cookies, then loads: from the
        /// network, or the markup the app supplies.
        func load(_ url: URL, serial: Int, in view: WKWebView) {
            if serial != self.serial { ended = 0 }
            self.serial = serial
            loaded = url
            shown = false
            let jar = (parent.client.effects as? SystemEffects)?.jar
            let content = parent.configuration.webContent
            let cookies = made.cookies
            Task { @MainActor in
                await jar?.ready(view.configuration.websiteDataStore.httpCookieStore, session: cookies)
                // The session ended meanwhile: this web view is on its way out.
                guard self.isCurrent else { return }
                switch content {
                case .network:
                    view.load(URLRequest(url: url))
                case .supplied(let supply):
                    let address = url.path.isEmpty ? "/" : url.path + (url.query.map { "?" + $0 } ?? "")
                    if let html = await supply(address) {
                        view.loadHTMLString(html, baseURL: url)
                    } else {
                        view.load(URLRequest(url: url))
                    }
                }
            }
        }

        /// Whether `url` is the app's own: on the configured origin, whole,
        /// whatever address the page was loaded from. What else may load is
        /// the core's to say.
        func isThePagesOwn(_ url: URL) -> Bool {
            parent.configuration.site?.inside(url.absoluteString) != nil
        }

        func webView(
            _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
        ) async -> WKNavigationActionPolicy {
            guard let url = navigationAction.request.url else { return .cancel }
            let mainFrame = navigationAction.targetFrame?.isMainFrame ?? true
            let decision = WebNavigationRules.decide(
                linkActivated: navigationAction.navigationType == .linkActivated, mainFrame: mainFrame,
                ours: isThePagesOwn(url), loaded: url == loaded, empty: url.scheme == "about"
            )
            switch decision {
            case .allow:
                return .allow
            case .tap:
                // A link the person tapped: the core decides where it leads.
                parent.client.send(.webLink(screen: parent.screen, url: url.absoluteString))
            case .cancel:
                // The page's script, a redirect or a form would leave the
                // origin: it goes nowhere, and nobody is told it was a tap.
                break
            }
            // A first page stopped on its way is a screen that failed, not a blank one.
            if mainFrame, !shown { parent.failed() }
            return .cancel
        }

        func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
            shown = true
        }

        func webView(
            _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error
        ) {
            // A first page that never arrived is a screen that failed; a load
            // the web view gave up for a newer one is not.
            let error = error as NSError
            if !shown, !(error.domain == NSURLErrorDomain && error.code == NSURLErrorCancelled) { parent.failed() }
        }

        /// The page's process ended (the system took its memory, or it
        /// crashed): the page is loaded again once; a second end is a screen
        /// that failed, never a blank one.
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            ended += 1
            guard ended == 1, let loaded, let serial else { return parent.failed() }
            load(loaded, serial: serial, in: webView)
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            // Only what a page of the origin set is taken into the session.
            guard let jar = (parent.client.effects as? SystemEffects)?.jar, let site = parent.configuration.site,
                let url = webView.url, isThePagesOwn(url)
            else { return }
            let cookies = made.cookies
            Task { @MainActor in
                await jar.adopt(
                    from: webView.configuration.websiteDataStore.httpCookieStore, host: site.host, session: cookies
                )
            }
        }

        func webView(
            _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
            for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
        ) -> WKWebView? {
            // A link the person tapped that asks for a new window is a link,
            // which the core follows; a window a script asks for is not
            // opened, and nobody is told.
            let linkActivated = navigationAction.navigationType == .linkActivated
            if WebNavigationRules.newWindow(linkActivated: linkActivated) == .tap, let url = navigationAction.request.url {
                parent.client.send(.webLink(screen: parent.screen, url: url.absoluteString))
            }
            return nil
        }
    }
}

#if canImport(UIKit)
extension WebPageView: UIViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeUIView(context: Context) -> WKWebView { makeWebView(context.coordinator) }
    func updateUIView(_ view: WKWebView, context: Context) {
        context.coordinator.parent = self
        if context.coordinator.serial != serial { context.coordinator.load(url, serial: serial, in: view) }
    }
}
#else
extension WebPageView: NSViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> WKWebView { makeWebView(context.coordinator) }
    func updateNSView(_ view: WKWebView, context: Context) {
        context.coordinator.parent = self
        if context.coordinator.serial != serial { context.coordinator.load(url, serial: serial, in: view) }
    }
}
#endif
