#if canImport(UIKit)
import UIKit

/// Each window scene's delegate, which ``NavreeAppDelegate`` gives the
/// system: a Home Screen quick action chosen at launch or while the app runs
/// opens its address, in the window in front or the first one ready.
///
/// It opens the app's own quick actions (type `navree.open`, the address in
/// `userInfo["href"]`): the dynamic ones the app sets from its shortcuts, and
/// static ones an app writes in its Info.plist (`UIApplicationShortcutItems`),
/// which work before the app's first launch.
@MainActor
open class NavreeSceneDelegate: NSObject, UIWindowSceneDelegate {
    open func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        if let item = options.shortcutItem { Self.perform(item) }
    }

    open func windowScene(
        _ windowScene: UIWindowScene, performActionFor shortcutItem: UIApplicationShortcutItem
    ) async -> Bool {
        Self.perform(shortcutItem)
    }

    /// Opens a quick action's address; false for one that is not the app's.
    @discardableResult
    static func perform(_ item: UIApplicationShortcutItem) -> Bool {
        let info = item.userInfo.map { Dictionary(uniqueKeysWithValues: $0.map { ($0.key, $0.value as Any) }) }
        guard let href = ShortcutRules.href(type: item.type, userInfo: info) else { return false }
        PresenceModel.shared.open(href)
        return true
    }
}
#endif
