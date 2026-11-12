import Foundation
import NavreeProtocol
import SwiftUI
import WebKit

/// A page of the app shown whole in the system web view, inside the app's own
/// bars: its title is the bar's, its links are the app's navigation.
///
/// What a page in it can do is what a browser tab on the same site can do,
/// and nothing more: there is no script bridge to the app, no pop-up, no file,
/// no address outside the app's origin (those open in the system browser).
struct WebPageView {
    let url: URL
    let ref: PageRef
    let client: NativeClient

    @MainActor
    func makeWebView(_ coordinator: Coordinator) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        configuration.applicationNameForUserAgent = Wire.userAgentToken(platform: Platform.platform, lib: Library.version)
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.defaultWebpagePreferences.allowsContentJavaScript = true
        // No `WKScriptMessageHandler` is ever added to the user content
        // controller: the page has no way to reach the app.
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = coordinator
        view.uiDelegate = coordinator
        view.allowsBackForwardNavigationGestures = false
        #if DEBUG
        view.isInspectable = true
        #else
        view.isInspectable = false
        #endif
        coordinator.observe(view)
        coordinator.load(url, in: view)
        return view
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        let parent: WebPageView
        private var title: NSKeyValueObservation?
        private var loaded: URL?

        init(_ parent: WebPageView) {
            self.parent = parent
        }

        var origin: Origin { parent.client.configuration.origin }

        func observe(_ view: WKWebView) {
            title = view.observe(\.title, options: [.new]) { [weak self] view, _ in
                MainActor.assumeIsolated {
                    guard let self, let title = view.title, !title.isEmpty else { return }
                    self.parent.client.send(.webTitle(title, at: self.parent.ref))
                }
            }
        }

        /// Copies the session in, then loads: from the network, or the markup
        /// the app supplies.
        func load(_ url: URL, in view: WKWebView) {
            loaded = url
            let client = parent.client
            Task { @MainActor in
                await client.jar.copy(into: view.configuration.websiteDataStore.httpCookieStore)
                switch client.configuration.webContent {
                case .network:
                    view.load(URLRequest(url: url))
                case .supplied(let supply):
                    let address = Origin.pathAndQuery(of: url)
                    if let html = await supply(address) {
                        view.loadHTMLString(html, baseURL: url)
                    } else {
                        view.load(URLRequest(url: url))
                    }
                }
            }
        }

        func webView(
            _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
        ) async -> WKNavigationActionPolicy {
            guard let url = navigationAction.request.url else { return .cancel }
            let isMainFrame = navigationAction.targetFrame?.isMainFrame ?? true
            if url.scheme == "about" && !isMainFrame { return .allow }
            switch origin.destination(of: url) {
            case .refused:
                return .cancel
            case .external(let target):
                if navigationAction.navigationType == .linkActivated || navigationAction.targetFrame == nil {
                    Platform.open(target)
                }
                return .cancel
            case .app(let address):
                if !isMainFrame { return .allow }
                if url == loaded && navigationAction.navigationType != .linkActivated { return .allow }
                if navigationAction.navigationType == .linkActivated {
                    // A link is the app's navigation: a native screen, pushed
                    // or presented as the shell's rules say.
                    parent.client.send(.webLink(address, at: parent.ref))
                    return .cancel
                }
                // A form posted, a redirect, the page's own reload: it stays.
                return .allow
            }
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            let jar = parent.client.jar
            Task { @MainActor in
                await jar.adopt(from: webView.configuration.websiteDataStore.httpCookieStore)
            }
        }

        func webView(
            _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
            for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
        ) -> WKWebView? {
            // A page that asks for a new window gets a navigation instead.
            if let url = navigationAction.request.url, case .app(let address) = origin.destination(of: url) {
                parent.client.send(.webLink(address, at: parent.ref))
            } else if let url = navigationAction.request.url, case .external(let target) = origin.destination(of: url) {
                Platform.open(target)
            }
            return nil
        }
    }
}

#if canImport(UIKit)
extension WebPageView: UIViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeUIView(context: Context) -> WKWebView { makeWebView(context.coordinator) }
    func updateUIView(_ view: WKWebView, context: Context) {}
}
#else
extension WebPageView: NSViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> WKWebView { makeWebView(context.coordinator) }
    func updateNSView(_ view: WKWebView, context: Context) {}
}
#endif
