/// The count on the app's icon, and a destination's badge.
public enum BadgeRules {
    /// A destination's badge in the tab bar and the sidebar: its count when
    /// the badge is a number, else none (zero).
    public static func tabBadge(_ badge: String?) -> Int {
        badge.flatMap { Int($0) } ?? 0
    }
}
