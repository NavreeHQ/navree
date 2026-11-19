import Accessibility
import CryptoKit
import Foundation
import NavreeProtocol
import NavreeRuntime
import SwiftUI
import Synchronization
import UniformTypeIdentifiers
import WebKit

#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// What presents the app's own file picker when a web page asks for files.
@MainActor
protocol WebFilePicker {
    func pick(multiple: Bool) async -> [URL]?
}

/// The system's own file browser for web `<input type="file">` fields:
/// `UIDocumentPickerViewController` on iPhone and iPad, `NSOpenPanel` on a Mac.
@MainActor
final class SystemWebFilePicker: WebFilePicker {
    #if canImport(UIKit)
    private var activeDelegate: DocumentPickerDelegate?
    #endif

    func pick(multiple: Bool) async -> [URL]? {
        #if canImport(UIKit)
        if let previous = activeDelegate {
            activeDelegate = nil
            previous.cancelAndDismiss()
        }
        guard let presenter = Windows.top else { return nil }
        return await withCheckedContinuation { continuation in
            let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.item])
            picker.allowsMultipleSelection = multiple
            let delegate = DocumentPickerDelegate(controller: picker, continuation: continuation)
            delegate.onComplete = { [weak self, weak delegate] in
                if let delegate, self?.activeDelegate === delegate {
                    self?.activeDelegate = nil
                }
            }
            self.activeDelegate = delegate
            picker.delegate = delegate
            picker.presentationController?.delegate = delegate
            presenter.present(picker, animated: true)
        }
        #elseif canImport(AppKit)
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = multiple
        let response: NSApplication.ModalResponse
        if let window = Windows.key {
            response = await panel.beginSheetModal(for: window)
        } else {
            response = panel.runModal()
        }
        return response == .OK && !panel.urls.isEmpty ? panel.urls : nil
        #endif
    }
}

#if canImport(UIKit)
@MainActor
private final class DocumentPickerDelegate: NSObject, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate {
    private let pending: Mutex<CheckedContinuation<[URL]?, Never>?>
    private weak var controller: UIDocumentPickerViewController?
    var onComplete: (@MainActor () -> Void)?

    init(
        controller: UIDocumentPickerViewController,
        continuation: CheckedContinuation<[URL]?, Never>
    ) {
        self.controller = controller
        self.pending = Mutex(continuation)
    }

    deinit {
        let leftover = pending.withLock { slot -> CheckedContinuation<[URL]?, Never>? in
            defer { slot = nil }
            return slot
        }
        leftover?.resume(returning: nil)
    }

    func cancelAndDismiss() {
        controller?.dismiss(animated: false)
        complete(nil)
    }

    private func complete(_ urls: [URL]?) {
        let current = pending.withLock { slot -> CheckedContinuation<[URL]?, Never>? in
            defer { slot = nil }
            return slot
        }
        let notify = onComplete
        onComplete = nil
        notify?()
        current?.resume(returning: urls)
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        complete(urls.isEmpty ? nil : urls)
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        complete(nil)
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        complete(nil)
    }
}
#endif

/// Tracks when the person last touched, clicked, pressed a key or activated
/// an element with assistive technology in the web view, so a script's own
/// `anchor.click()` is not mistaken for a tap.
struct InteractionTracker: Sendable {
    static let window: Duration = .seconds(1)
    private(set) var last: ContinuousClock.Instant?

    mutating func note(at instant: ContinuousClock.Instant = ContinuousClock.now) {
        last = instant
    }

    func hasRecent(at instant: ContinuousClock.Instant = ContinuousClock.now) -> Bool {
        guard let last else { return false }
        let elapsed = last.duration(to: instant)
        return elapsed >= .zero && elapsed <= Self.window
    }
}

/// A `WKWebView` that observes real person input without interfering with
/// WebKit's own gestures or scrolling.
@MainActor
final class InteractiveWebView: WKWebView {
    var onInteraction: (() -> Void)?

    #if canImport(UIKit)
    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        let hit = super.hitTest(point, with: event)
        if hit != nil { onInteraction?() }
        return hit
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        onInteraction?()
        super.pressesBegan(presses, with: event)
    }

    override func accessibilityActivate() -> Bool {
        onInteraction?()
        return super.accessibilityActivate()
    }
    #elseif canImport(AppKit)
    private nonisolated(unsafe) var eventMonitor: Any?

    deinit {
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
        }
    }

    func startMonitoring() {
        guard eventMonitor == nil else { return }
        eventMonitor = NSEvent.addLocalMonitorForEvents(
            matching: [.leftMouseDown, .leftMouseUp, .rightMouseDown, .otherMouseDown, .keyDown]
        ) { [weak self] event in
            MainActor.assumeIsolated {
                if let self, let window = self.window, event.window === window {
                    self.onInteraction?()
                }
            }
            return event
        }
    }

    func stopMonitoring() {
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
            self.eventMonitor = nil
        }
    }

    override func accessibilityPerformPress() -> Bool {
        onInteraction?()
        return super.accessibilityPerformPress()
    }
    #endif
}

/// A page shown whole in the system web view, inside the app's own bars, at
/// the address the runtime core gives, loaded again whenever its serial moves.
///
/// What a page in it can do is what a browser tab on the same site can do, and
/// nothing more: there is no script bridge to the app, no pop-up, and no way
/// to the app's files. Where the system gives ``WKUIDelegate``'s open-panel
/// call (macOS 10.12 and later, iOS and iPadOS 18.4 and later — including
/// iOS 26, iPadOS 26 and macOS 26), a web page's `<input type="file">` opens
/// the app's own file picker and passes every chosen file through ``PhotoPlace``
/// and ``FilmPlace`` with the same refusal words as a native field, while file
/// drops and file pastes are stopped; elsewhere `<input type="file">` is
/// switched off at document start and tells the person to use the app's own
/// form. A link the person actually taps is the core's to follow; what a page
/// does by itself (including `anchor.click()`) stays on the origin or goes
/// nowhere, and is never told to the core as a tap.
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
        if let site = configuration.site {
            let answersFilePicker = WebFileRules.systemAnswersOpenPanel
            settings.userContentController.addUserScript(WKUserScript(
                source: WebRules.documentScript(
                    site: site, hosts: configuration.webHosts, answersFilePicker: answersFilePicker
                ),
                injectionTime: .atDocumentStart, forMainFrameOnly: false, in: .defaultClient
            ))
            settings.userContentController.addUserScript(WKUserScript(
                source: WebRules.pageClickScript(site: site, answersFilePicker: answersFilePicker),
                injectionTime: .atDocumentStart, forMainFrameOnly: false, in: .page
            ))
        }
        let view = InteractiveWebView(frame: .zero, configuration: settings)
        view.onInteraction = { [weak coordinator] in coordinator?.noteInteraction() }
        #if canImport(AppKit)
        view.startMonitoring()
        #endif
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
        var filePicker: any WebFilePicker = SystemWebFilePicker()
        var filmExporter: FilmPlace.Exporter = FilmPlace.systemExport
        private(set) var interactions = InteractionTracker()
        private var title: NSKeyValueObservation?
        private var loading: Task<Void, Never>?
        private var panelTask: Task<Void, Never>?
        private var pendingPanel: (@MainActor ([URL]?) -> Void)?
        private var panelSerial = 0
        private var dismantled = false
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

        func noteInteraction(at instant: ContinuousClock.Instant = ContinuousClock.now) {
            interactions.note(at: instant)
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
            guard !dismantled else { return }
            loading?.cancel()
            if serial != self.serial { ended = 0 }
            self.serial = serial
            loaded = url
            shown = false
            let jar = (parent.client.effects as? SystemEffects)?.jar
            let content = parent.configuration.webContent
            let cookies = made.cookies
            loading = Task { @MainActor in
                // Nothing loads before the rules of what a page may load are
                // in place: a page that cannot be held to them is a failed one.
                guard !self.dismantled else { return }
                guard await self.rule(view) else { return self.parent.failed() }
                guard !self.dismantled else { return }
                await jar?.ready(view.configuration.websiteDataStore.httpCookieStore, session: cookies)
                // The session ended or the view was taken down meanwhile.
                guard !self.dismantled, self.isCurrent else { return }
                switch content {
                case .network:
                    view.load(URLRequest(url: url))
                case .supplied(let supply):
                    let address = url.path.isEmpty ? "/" : url.path + (url.query.map { "?" + $0 } ?? "")
                    let html = await supply(address)
                    guard !self.dismantled, self.isCurrent else { return }
                    if let html {
                        view.loadHTMLString(html, baseURL: url)
                    } else {
                        view.load(URLRequest(url: url))
                    }
                }
            }
        }

        /// Stops the page and detaches the view when it leaves the window
        /// (such as while a sign-in cover stands over the screen).
        func tearDown(_ view: WKWebView) {
            dismantled = true
            loading?.cancel()
            loading = nil
            cancelPendingPanel()
            title?.invalidate()
            title = nil
            if let interactive = view as? InteractiveWebView {
                interactive.onInteraction = nil
                #if canImport(AppKit)
                interactive.stopMonitoring()
                #endif
            }
            view.stopLoading()
            view.navigationDelegate = nil
            view.uiDelegate = nil
            if let blank = URL(string: "about:blank") {
                view.load(URLRequest(url: blank))
            }
        }

        private func cancelPendingPanel() {
            panelSerial &+= 1
            panelTask?.cancel()
            panelTask = nil
            guard let handler = pendingPanel else { return }
            pendingPanel = nil
            handler(nil)
        }

        private func finishPanel(serial: Int, with urls: [URL]?) {
            guard panelSerial == serial, let handler = pendingPanel else { return }
            pendingPanel = nil
            panelTask = nil
            handler(urls)
        }

        /// Answers a web page's `<input type="file">` request through the
        /// app's own picker, ``PhotoPlace``, ``FilmPlace`` and ``FileNotes``.
        func handleOpenPanel(
            multiple: Bool,
            allowsDirectories: Bool,
            frameURL: URL?,
            mainFrame: Bool,
            completionHandler: @escaping @MainActor ([URL]?) -> Void
        ) {
            cancelPendingPanel()
            panelSerial &+= 1
            let serial = panelSerial
            pendingPanel = completionHandler

            let config = parent.configuration
            guard !dismantled, isCurrent,
                !ModalRules.isUnderLockedCover(parent.screen, in: parent.client.nav),
                WebFileRules.allowsPanel(
                    frameURL: frameURL, mainFrame: mainFrame, site: config.site,
                    hosts: config.webHosts, directoryRequested: allowsDirectories
                )
            else {
                finishPanel(serial: serial, with: nil)
                return
            }

            panelTask = Task { @MainActor [weak self] in
                guard let self else { return }
                let chosen = await self.filePicker.pick(multiple: multiple)
                guard self.panelSerial == serial, !Task.isCancelled else { return }
                guard let chosen, !chosen.isEmpty,
                    !self.dismantled, self.isCurrent,
                    !ModalRules.isUnderLockedCover(self.parent.screen, in: self.parent.client.nav)
                else {
                    self.finishPanel(serial: serial, with: nil)
                    return
                }
                let effects = self.parent.client.effects as? SystemEffects
                let store = effects?.keeper.files ?? .shared
                let prepared = await store.prepareForWeb(
                    chosen, multiple: multiple, filmExporter: self.filmExporter
                )
                guard self.panelSerial == serial, !Task.isCancelled else {
                    for url in prepared.kept {
                        try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
                    }
                    return
                }
                guard !self.dismantled, self.isCurrent,
                    !ModalRules.isUnderLockedCover(self.parent.screen, in: self.parent.client.nav)
                else {
                    for url in prepared.kept {
                        try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
                    }
                    self.finishPanel(serial: serial, with: nil)
                    return
                }
                effects?.fileNotes.sayOnWeb(prepared.refused, screen: self.parent.screen)
                if !prepared.refused.isEmpty {
                    AccessibilityNotification.Announcement(prepared.refused.joined(separator: " ")).post()
                }
                self.finishPanel(serial: serial, with: prepared.kept.isEmpty ? nil : prepared.kept)
            }
        }

        /// Whether the content rules are in the view.
        private var ruled = false

        /// Puts the content rules in the view once: what a page may load
        /// inside it (``WebRules``). `false` when they could not be made, and
        /// then the page is not loaded.
        private func rule(_ view: WKWebView) async -> Bool {
            if ruled { return true }
            guard let site = parent.configuration.site, let store = WKContentRuleListStore.default() else {
                return false
            }
            let source = WebRules.contentRules(site: site, hosts: parent.configuration.webHosts)
            let name =
                "navree-web-" + SHA256.hash(data: Data(source.utf8)).map { String(format: "%02x", $0) }.joined()
            guard let list = try? await store.compileContentRuleList(forIdentifier: name, encodedContentRuleList: source)
            else { return false }
            view.configuration.userContentController.add(list)
            ruled = true
            return true
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
            // The page itself is always the origin's own; a frame inside it
            // may also be of a host the app names for the web view.
            let configuration = parent.configuration
            let ours = mainFrame
                ? isThePagesOwn(url)
                : WebRules.allowsFrame(url, site: configuration.site, hosts: configuration.webHosts)
            let decision = WebNavigationRules.decide(
                linkActivated: navigationAction.navigationType == .linkActivated, mainFrame: mainFrame,
                ours: ours, loaded: url == loaded, empty: url.scheme == "about",
                userInteracted: interactions.hasRecent()
            )
            switch decision {
            case .allow:
                return .allow
            case .tap:
                // A link the person actually tapped: the core decides where it leads.
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

        @available(iOS 18.4, macOS 10.12, *)
        func webView(
            _ webView: WKWebView,
            runOpenPanelWith parameters: WKOpenPanelParameters,
            initiatedByFrame frame: WKFrameInfo,
            completionHandler: @escaping @MainActor @Sendable ([URL]?) -> Void
        ) {
            // Use the frame's security origin alone so sandboxed opaque frames
            // (`host == ""`) and `about:` frames never open a picker.
            handleOpenPanel(
                multiple: parameters.allowsMultipleSelection,
                allowsDirectories: parameters.allowsDirectories,
                frameURL: Self.originURL(of: frame.securityOrigin),
                mainFrame: frame.isMainFrame,
                completionHandler: { urls in completionHandler(urls) }
            )
        }

        private static func originURL(of securityOrigin: WKSecurityOrigin) -> URL? {
            let scheme = securityOrigin.protocol.lowercased()
            let host = securityOrigin.host
            guard !scheme.isEmpty, !host.isEmpty else { return nil }
            let portPart = securityOrigin.port > 0 ? ":\(securityOrigin.port)" : ""
            return URL(string: "\(scheme)://\(host)\(portPart)/")
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
            // A link the person actually tapped that asks for a new window is a
            // link, which the core follows; a window a script asks for is not
            // opened, and nobody is told.
            let linkActivated = navigationAction.navigationType == .linkActivated
            let decision = WebNavigationRules.newWindow(
                linkActivated: linkActivated, userInteracted: interactions.hasRecent()
            )
            if decision == .tap, let url = navigationAction.request.url {
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
    static func dismantleUIView(_ view: WKWebView, coordinator: Coordinator) {
        coordinator.tearDown(view)
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
    static func dismantleNSView(_ view: WKWebView, coordinator: Coordinator) {
        coordinator.tearDown(view)
    }
}
#endif
