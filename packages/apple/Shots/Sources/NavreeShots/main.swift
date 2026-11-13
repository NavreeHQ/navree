// Draws recorded answers as the client draws them, in light and dark, at an
// iPhone's width and an iPad's, and writes each as PNG pictures:
//
//   NavreeShots <out folder> <answer.json>...
//
// For each answer: `<name>-<width>-<light|dark>-top.png`, the screen as it
// first shows (its bar, its title, what fits under them), and `-whole.png`,
// the whole screen from its bar to the end of its content. An answer whose
// file name ends in `-phone` is drawn 402 points wide, one ending in
// `-tablet` 834 points wide, any other at both. Nothing is sent anywhere:
// the drawing's intents go nowhere.
import NavreeDesign
import NavreeProtocol
import SwiftUI
import UIKit

struct Shot {
    let file: URL
    let width: CGFloat
    let dark: Bool

    var name: String { file.deletingPathExtension().lastPathComponent }
    var height: CGFloat { width < 600 ? 874 : 1194 }
    var scale: CGFloat { width < 600 ? 3 : 2 }
    var look: String { dark ? "dark" : "light" }
}

enum Plan {
    static func widths(for file: URL) -> [CGFloat] {
        let name = file.deletingPathExtension().lastPathComponent
        if name.hasSuffix("-phone") { return [402] }
        if name.hasSuffix("-tablet") { return [834] }
        return [402, 834]
    }

    static func shots(_ files: [URL]) -> [Shot] {
        files.flatMap { file in
            widths(for: file).flatMap { width in [false, true].map { Shot(file: file, width: width, dark: $0) } }
        }
    }
}

/// The screen as an app draws it at the root of a destination.
struct Screen: View {
    let envelope: Navree.Envelope

    var body: some View {
        NavigationStack {
            if let screen = envelope.screen {
                ScreenView(screen: screen, depth: .root)
            } else {
                Text("This answer holds no screen.")
            }
        }
        .environment(\.navree, DrawingContext(send: { _ in }, icons: envelope.icons))
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
                try await take(shot, envelope: envelope)
            } catch {
                FileHandle.standardError.write(Data("\(shot.file.lastPathComponent): \(error)\n".utf8))
            }
        }
    }

    private func take(_ shot: Shot, envelope: Navree.Envelope) async throws {
        let host = UIHostingController(rootView: Screen(envelope: envelope))
        host.overrideUserInterfaceStyle = shot.dark ? .dark : .light
        host.traitOverrides.horizontalSizeClass = shot.width < 600 ? .compact : .regular
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

    /// The scroll view that holds the screen's content: the largest one.
    static func scrollView(in view: UIView) -> UIScrollView? {
        var found: [UIScrollView] = []
        var pending: [UIView] = [view]
        while let next = pending.popLast() {
            if let scroll = next as? UIScrollView, scroll.bounds.width > view.bounds.width * 0.6 { found.append(scroll) }
            pending.append(contentsOf: next.subviews)
        }
        return found.max { $0.bounds.height < $1.bounds.height }
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
        let files = arguments.dropFirst().map { URL(fileURLWithPath: $0) }

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
