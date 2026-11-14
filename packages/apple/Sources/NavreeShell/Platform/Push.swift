import Foundation
import UserNotifications
import os

#if canImport(UIKit)
import UIKit
#endif

/// What asks the person to allow notifications: the system, or a stand-in in tests.
@MainActor
protocol NotificationPermission: AnyObject {
    /// `done` gets what the person said, now or before.
    func ask(done: @escaping @MainActor (Bool) -> Void)
}

/// The system's own prompt, shown once; later asks answer what was said.
@MainActor
final class SystemNotificationPermission: NotificationPermission {
    func ask(done: @escaping @MainActor (Bool) -> Void) {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { granted, _ in
            Task { @MainActor in done(granted) }
        }
    }
}

#if canImport(UIKit)
/// What the system tells the app outside any view, passed on to the shell:
/// this install's push token, a notification the person tapped, and (through
/// ``NavreeSceneDelegate``) a Home Screen quick action. An app
/// adopts it in its `App`:
///
/// ```swift
/// @UIApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
/// ```
///
/// The token is asked for only by a build signed for push
/// (`aps-environment`), at every launch and once the person allows
/// notifications; the core posts it to the server, never the app.
@MainActor
open class NavreeAppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    static let log = Logger(subsystem: "com.navree.native", category: "push")

    open func application(
        _ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        return true
    }

    /// Each window's scene gets the shell's scene delegate, which hands a
    /// Home Screen quick action to the app.
    open func application(
        _ application: UIApplication, configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: nil, sessionRole: session.role)
        if session.role == .windowApplication { configuration.delegateClass = NavreeSceneDelegate.self }
        return configuration
    }

    open func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data) {
        AppEvents.received(pushToken: token)
    }

    open func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: any Error) {
        Self.log.error("no push token: \(error.localizedDescription, privacy: .public)")
    }

    /// A notification tapped: the address it carries (`href`, beside `aps`) is
    /// opened as the core says.
    nonisolated open func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        guard let href = AppEvents.href(response.notification.request.content.userInfo) else { return }
        await MainActor.run { AppEvents.opened(notification: href) }
    }

    /// A notification that arrives while the app is in front is shown as the
    /// system shows it, not swallowed.
    nonisolated open func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .list, .sound]
    }
}
#endif

extension AppEvents {
    /// The address a notification carries: `href` beside `aps`, as the
    /// server's push writes it. The core reads it as any link from outside.
    nonisolated static func href(_ payload: [AnyHashable: Any]) -> String? {
        guard let href = payload["href"] as? String, !href.isEmpty else { return nil }
        return href
    }
}
