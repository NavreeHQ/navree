// Draws recorded answers as the client draws them, in light and dark, at an
// iPhone's width and an iPad's, and writes each as PNG pictures:
//
//   NavreeShots <out folder> <answer.json>...
//
// For each answer: `<name>-<width>-<light|dark>-top.png`, the screen as it
// first shows (its bar, its title, what fits under them), and `-whole.png`,
// the whole screen from its bar to the end of its content. An answer whose
// file name ends in `-phone` is drawn 402 points wide, one ending in
// `-tablet` 834 points wide, any other at both; `SHOT_WIDTHS=507,834,1024,1366`
// draws every answer at each of those widths instead, each at the height of
// that window (a narrow Stage Manager window, an 11-inch iPad upright, a
// 13-inch one upright and across). The window's class follows the width as
// the app tells the core: compact under 600 points, medium under 1000.
//
// A screen of panes led by a list is drawn as the app draws it, in the
// system's split view; `<list.json>+<detail.json>` draws a list whose rows
// open screens beside one of them. Nothing is sent anywhere: the drawing's
// intents go nowhere. A Mac has no Dynamic Type, so the pictures are at the
// default text size: larger sizes need a simulator.
import NavreeDesign
import NavreeProtocol
import SwiftUI
import UIKit

struct Shot {
    let file: URL
    /// The screen a list's row opened, drawn beside the list.
    let detail: URL?
    let width: CGFloat
    let dark: Bool

    var name: String {
        let list = file.deletingPathExtension().lastPathComponent
        return detail.map { list + "+" + $0.deletingPathExtension().lastPathComponent } ?? list
    }
    var height: CGFloat {
        switch width {
        case ..<450: 874
        case ..<600: 1000
        case 1366: 1024
        case 1024: 1366
        default: 1194
        }
    }
    var scale: CGFloat { width < 450 ? 3 : 2 }
    var look: String { dark ? "dark" : "light" }
    var window: WindowClass { WindowClass(sizeClass: width < 600 ? .compact : .regular, points: Double(width)) }
}

enum Plan {
    static func widths(for file: URL) -> [CGFloat] {
        if let given = ProcessInfo.processInfo.environment["SHOT_WIDTHS"] {
            let widths = given.split(separator: ",").compactMap { Double($0) }.map { CGFloat($0) }
            if !widths.isEmpty { return widths }
        }
        let name = file.deletingPathExtension().lastPathComponent
        if name.hasSuffix("-phone") { return [402] }
        if name.hasSuffix("-tablet") { return [834] }
        return [402, 834]
    }

    static func shots(_ arguments: [String]) -> [Shot] {
        arguments.flatMap { argument -> [Shot] in
            let parts = argument.split(separator: "+").map { URL(fileURLWithPath: String($0)) }
            guard let file = parts.first else { return [] }
            let detail = parts.count > 1 ? parts[1] : nil
            return widths(for: file).flatMap { width in
                [false, true].map { Shot(file: file, detail: detail, width: width, dark: $0) }
            }
        }
    }
}

/// The screen as an app draws it at the root of a destination: in a
/// navigation stack, or as a list beside its detail where the app's stack
/// would be columns at this width.
struct Screen: View {
    let envelope: Navree.Envelope
    let detail: Navree.Envelope?
    let window: WindowClass
    /// As an app's stack has one, so a card that zooms is drawn as a source.
    @Namespace private var zoom

    var body: some View {
        Group {
            if let screen = envelope.screen {
                switch SplitRules.layout(screen.body, width: window) {
                case .panes(let panes):
                    SplitColumns(
                        path: .constant([]), compactColumn: SplitRules.compactColumn(panes),
                        inspectorTitle: panes.inspector?.title, choose: { _, _ in },
                        list: { ScreenView(screen: SplitRules.screen(.list, of: screen, panes: panes), depth: .root) },
                        detail: { ScreenView(screen: SplitRules.screen(.detail, of: screen, panes: panes), depth: .pushed) },
                        inspector: panes.inspector == nil ? nil : {
                            ScreenView(screen: SplitRules.screen(.inspector, of: screen, panes: panes), depth: .pushed)
                        },
                        pushed: { _ in EmptyView() }
                    )
                case .listBesideDetail where detail != nil:
                    SplitColumns(
                        path: .constant([]), compactColumn: .detail, choose: { _, _ in },
                        list: { ScreenView(screen: screen, depth: .root) },
                        detail: {
                            if let opened = detail?.screen { ScreenView(screen: opened, depth: .pushed) } else { NothingChosen() }
                        },
                        pushed: { _ in EmptyView() }
                    )
                default:
                    NavigationStack {
                        ScreenView(screen: screen, depth: .root)
                    }
                }
            } else {
                Text("This answer holds no screen.")
            }
        }
        .environment(\.navree, DrawingContext(send: { _ in }, icons: envelope.icons))
        .environment(\.navreeZoomSpace, zoom)
        .environment(\.navreeWindow, window)
    }

    /// Whether the screen is drawn as columns, which have no one scroll to
    /// draw whole.
    static func columns(_ envelope: Navree.Envelope, detail: Navree.Envelope?, window: WindowClass) -> Bool {
        guard let screen = envelope.screen else { return false }
        switch SplitRules.layout(screen.body, width: window) {
        case .panes: return true
        case .listBesideDetail: return detail != nil
        case .stack: return false
        }
    }
}

@MainActor
final class Shooter {
    let window: UIWindow
    let out: URL

    init(window: UIWindow, out: URL) {
        self.window = window
        self.out = out
    }

    func run(_ shots: [Shot]) async {
        for shot in shots {
            do {
                let data = try Data(contentsOf: shot.file)
                let envelope = try WireDecoder.decode(Navree.Envelope.self, from: data)
                let detail = try shot.detail.map { try WireDecoder.decode(Navree.Envelope.self, from: Data(contentsOf: $0)) }
                try await take(shot, envelope: envelope, detail: detail)
            } catch {
                FileHandle.standardError.write(Data("\(shot.file.lastPathComponent): \(error)\n".utf8))
            }
        }
    }

    private func take(_ shot: Shot, envelope: Navree.Envelope, detail: Navree.Envelope?) async throws {
        let host = UIHostingController(rootView: Screen(envelope: envelope, detail: detail, window: shot.window))
        host.overrideUserInterfaceStyle = shot.dark ? .dark : .light
        host.traitOverrides.horizontalSizeClass = shot.window == .compact ? .compact : .regular
        host.traitOverrides.verticalSizeClass = .regular
        guard let root = window.rootViewController else { return }
        root.addChild(host)
        root.view.addSubview(host.view)
        host.didMove(toParent: root)
        defer {
            host.willMove(toParent: nil)
            host.view.removeFromSuperview()
            host.removeFromParent()
        }

        host.view.frame = CGRect(x: 0, y: 0, width: shot.width, height: shot.height)
        try await settle(host.view)
        try write(host.view, to: "\(shot.name)-\(Int(shot.width))-\(shot.look)-top.png", scale: shot.scale)
        if Screen.columns(envelope, detail: detail, window: shot.window) { return }

        // The whole screen: as tall as its scrolling content, so nothing scrolls.
        for _ in 0..<3 {
            guard let scroll = Self.scrollView(in: host.view) else { break }
            let insets = scroll.adjustedContentInset
            let frame = scroll.convert(scroll.bounds, to: host.view)
            let tall = frame.minY + insets.top + scroll.contentSize.height + insets.bottom + (host.view.bounds.height - frame.maxY)
            if abs(tall - host.view.bounds.height) < 1 { break }
            host.view.frame.size.height = max(tall, shot.height)
            try await settle(host.view)
        }
        try write(host.view, to: "\(shot.name)-\(Int(shot.width))-\(shot.look)-whole.png", scale: shot.scale)
    }

    private func settle(_ view: UIView) async throws {
        view.setNeedsLayout()
        view.layoutIfNeeded()
        try await Task.sleep(for: .milliseconds(700))
        view.layoutIfNeeded()
    }

    private func write(_ view: UIView, to name: String, scale: CGFloat) throws {
        let format = UIGraphicsImageRendererFormat()
        format.scale = scale
        format.opaque = true
        let image = UIGraphicsImageRenderer(bounds: view.bounds, format: format).image { context in
            if ProcessInfo.processInfo.environment["SHOT_LAYERS"] == "1" {
                view.layer.render(in: context.cgContext)
            } else {
                view.drawHierarchy(in: view.bounds, afterScreenUpdates: true)
            }
        }
        guard let png = image.pngData() else { return }
        try png.write(to: out.appendingPathComponent(name))
        print("wrote \(name) \(Int(view.bounds.width))x\(Int(view.bounds.height))")
    }

    /// The scroll view that holds the screen's content: the one with the most
    /// to scroll through.
    static func scrollView(in view: UIView) -> UIScrollView? {
        var found: [UIScrollView] = []
        var pending: [UIView] = [view]
        while let next = pending.popLast() {
            if let scroll = next as? UIScrollView, scroll.bounds.width > view.bounds.width * 0.6 { found.append(scroll) }
            pending.append(contentsOf: next.subviews)
        }
        return found.max { $0.contentSize.height < $1.contentSize.height }
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let arguments = Array(CommandLine.arguments.dropFirst())
        guard arguments.count >= 2 else {
            FileHandle.standardError.write(Data("usage: NavreeShots <out folder> <answer.json>...\n".utf8))
            exit(2)
        }
        let out = URL(fileURLWithPath: arguments[0], isDirectory: true)
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let files = Array(arguments.dropFirst())

        scene.sizeRestrictions?.minimumSize = CGSize(width: 420, height: 600)
        let window = UIWindow(windowScene: scene)
        window.rootViewController = UIViewController()
        window.makeKeyAndVisible()
        self.window = window
        Task { @MainActor in
            await Shooter(window: window, out: out).run(Plan.shots(files))
            exit(0)
        }
    }
}

final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: nil, sessionRole: connectingSceneSession.role)
        configuration.delegateClass = SceneDelegate.self
        return configuration
    }
}

UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(AppDelegate.self))
