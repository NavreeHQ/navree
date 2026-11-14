import SwiftUI

/// The names of the app's scenes.
public enum NavreeScene {
    /// The main window group, which New Window opens.
    public static let main = "main"
}

#if os(macOS)
/// The app's scenes: its main windows, its Settings window and its item in the
/// menu bar, each as the configuration and the person's choices say.
///
///     var body: some Scene { NavreeScenes(configuration: .shop) }
///     var body: some Scene { NavreeScenes(configuration: .shop, toolbarStyle: .unifiedCompact) }
///
/// The window toolbar's style is the system's own (`.automatic`, `.unified`,
/// `.unifiedCompact`, `.expanded`, with or without the title), chosen in code
/// as the rest of the app's build is. `content` wraps each window's shell, for
/// what the app adds around it.
public struct NavreeScenes<ToolbarStyle: WindowToolbarStyle, Content: View>: Scene {
    let configuration: ShellConfiguration
    let toolbarStyle: ToolbarStyle
    let content: @MainActor (AppShell) -> Content

    public init(
        configuration: ShellConfiguration, toolbarStyle: ToolbarStyle,
        @ViewBuilder content: @escaping @MainActor (AppShell) -> Content
    ) {
        self.configuration = configuration
        self.toolbarStyle = toolbarStyle
        self.content = content
    }

    public var body: some Scene {
        NavreeMainWindow(configuration: configuration, content: content)
            .windowToolbarStyle(toolbarStyle)
        // The person's settings, from the App menu (⌘,), in a window of their own.
        Settings {
            SettingsShell(configuration: configuration)
        }
        NavreeStatusItem(configuration: configuration)
    }
}

extension NavreeScenes where ToolbarStyle == DefaultWindowToolbarStyle {
    public init(configuration: ShellConfiguration, @ViewBuilder content: @escaping @MainActor (AppShell) -> Content) {
        self.init(configuration: configuration, toolbarStyle: .automatic, content: content)
    }
}

extension NavreeScenes where Content == AppShell {
    public init(configuration: ShellConfiguration, toolbarStyle: ToolbarStyle) {
        self.init(configuration: configuration, toolbarStyle: toolbarStyle) { $0 }
    }
}

extension NavreeScenes where ToolbarStyle == DefaultWindowToolbarStyle, Content == AppShell {
    public init(configuration: ShellConfiguration) {
        self.init(configuration: configuration, toolbarStyle: .automatic) { $0 }
    }
}
#else
/// The app's scenes: its main windows.
///
///     var body: some Scene { NavreeScenes(configuration: .shop) }
///
/// `content` wraps each window's shell, for what the app adds around it.
public struct NavreeScenes<Content: View>: Scene {
    let configuration: ShellConfiguration
    let content: @MainActor (AppShell) -> Content

    public init(configuration: ShellConfiguration, @ViewBuilder content: @escaping @MainActor (AppShell) -> Content) {
        self.configuration = configuration
        self.content = content
    }

    public var body: some Scene {
        NavreeMainWindow(configuration: configuration, content: content)
    }
}

extension NavreeScenes where Content == AppShell {
    public init(configuration: ShellConfiguration) {
        self.init(configuration: configuration) { $0 }
    }
}
#endif

/// The main window group: the app's shell, the menu bar's commands, the
/// windows' look and where the app stands, as the configuration says. An app
/// that declares scenes of its own puts it beside them.
public struct NavreeMainWindow<Content: View>: Scene {
    let configuration: ShellConfiguration
    let content: @MainActor (AppShell) -> Content

    public init(configuration: ShellConfiguration, @ViewBuilder content: @escaping @MainActor (AppShell) -> Content) {
        self.configuration = configuration
        self.content = content
    }

    public var body: some Scene {
        WindowGroup(id: NavreeScene.main) {
            content(AppShell(configuration: configuration))
        }
        .commands { NavreeCommands(window: NavreeScene.main, configuration: configuration) }
        .navreeWindowLook(configuration.windows)
        .navreePresence(configuration.presence)
    }
}

extension NavreeMainWindow where Content == AppShell {
    public init(configuration: ShellConfiguration) {
        self.init(configuration: configuration) { $0 }
    }
}
