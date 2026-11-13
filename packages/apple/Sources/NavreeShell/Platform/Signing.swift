import Foundation

/// What a build is signed with, as its Info.plist says. An entitlement is
/// granted by the signature, which an app cannot read back on iPhone, so the
/// build says it once more, in the same place it is set: the build setting
/// `NAVREE_SIGNED_WITH` written into the Info.plist key `NavreeSignedWith`
/// (words apart: `associated-domains sign-in-with-apple push`), and the
/// domain into `NavreeAssociatedDomain`. A build that says nothing, as an
/// unsigned one, offers none of them.
extension Entitlement {
    /// The Info.plist key that lists what the build is signed with.
    public static let infoKey = "NavreeSignedWith"
    /// The Info.plist key that names the build's associated domain.
    public static let domainKey = "NavreeAssociatedDomain"

    /// Each entitlement by its word in `NavreeSignedWith`.
    static let words: [String: Entitlement] = [
        "associated-domains": .associatedDomains,
        "sign-in-with-apple": .signInWithApple,
        "push": .pushNotifications,
    ]

    /// What `bundle` says it is signed with, for an app whose origin is `origin`.
    public static func declared(in bundle: Bundle = .main, origin: String) -> Set<Entitlement> {
        declared(
            words: bundle.object(forInfoDictionaryKey: infoKey) as? String,
            domain: bundle.object(forInfoDictionaryKey: domainKey) as? String,
            origin: origin
        )
    }

    /// What the words say. The associated domain counts only when it is the
    /// origin's own host over `https`: passkeys, a confirmation and a sign-in
    /// that comes back on a link all happen there.
    static func declared(words: String?, domain: String?, origin: String) -> Set<Entitlement> {
        var found = Set(
            (words ?? "").split(whereSeparator: { $0 == " " || $0 == "," }).compactMap { Self.words[$0.lowercased()] }
        )
        // `example.in?mode=developer` names the domain with how the system checks it.
        let domain = domain?.split(separator: "?").first.map { $0.lowercased() }
        let url = URL(string: origin)
        let host = url?.scheme?.lowercased() == "https" ? url?.host?.lowercased() : nil
        if host == nil || domain != host { found.remove(.associatedDomains) }
        return found
    }
}
