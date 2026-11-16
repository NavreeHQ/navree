import Foundation
import NavreeRuntime
import Security
import Testing

@testable import NavreeShell

/// A clipboard that keeps what was put on it, and how.
@MainActor
final class PlayedClipboard: Clipboard {
    var copies: [(text: String, options: CopyOptions)] = []

    func put(_ text: String, options: CopyOptions) {
        copies.append((text, options))
    }
}

/// Where the session and a copied secret are kept, and how.
@MainActor
struct SecretsTests {
    @Test func theSessionsKeychainItemIsOfThisDeviceOnlyOnEverySystem() {
        let slot = KeychainSlot(service: CookieJar.service, account: "https://shop.example")
        // The data protection keychain on a Mac too: without this key a Mac
        // keeps the item in the login keychain's file, where the class below
        // does not apply.
        #expect(slot.query[kSecUseDataProtectionKeychain as String] as? Bool == true)
        #expect(slot.query[kSecAttrService as String] as? String == "com.navree.native.session")
        #expect(slot.query[kSecAttrAccount as String] as? String == "https://shop.example")
        // Never marked to synchronise, and readable after the first unlock on this device only.
        #expect(slot.query[kSecAttrSynchronizable as String] == nil)
        #expect(KeychainSlot.kept[kSecAttrSynchronizable as String] == nil)
        #expect(
            KeychainSlot.kept[kSecAttrAccessible as String] as? String
                == kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String
        )
    }

    @Test func aBuildWithNoKeychainOfItsOwnHasNothingLeftInOne() {
        #expect(KeychainSlot.isGone(errSecSuccess))
        #expect(KeychainSlot.isGone(errSecItemNotFound))
        // A Mac build with no application identifier: the system keeps nothing for it.
        #expect(KeychainSlot.isGone(errSecMissingEntitlement))
        // The device is locked, or the system refused: the item may still be there.
        #expect(!KeychainSlot.isGone(errSecInteractionNotAllowed))
        #expect(!KeychainSlot.isGone(errSecAuthFailed))
    }

    @Test func aSecretIsCopiedForThisDeviceOnlyAndForAMinute() throws {
        #expect(ClipboardRules.options(sensitive: true) == CopyOptions(localOnly: true, expires: 60))
        #expect(ClipboardRules.options(sensitive: false) == CopyOptions(localOnly: false, expires: nil))
        let shell = try Shell()
        let clipboard = PlayedClipboard()
        shell.effects.clipboard = clipboard
        shell.effects.run(.copy(text: "4K7Q-ZP2M-9XTD", sensitive: true), for: shell.client)
        shell.effects.run(.copy(text: "CS-20481", sensitive: false), for: shell.client)
        #expect(clipboard.copies.map(\.text) == ["4K7Q-ZP2M-9XTD", "CS-20481"])
        #expect(clipboard.copies.map(\.options) == [CopyOptions(localOnly: true, expires: 60), CopyOptions()])
    }
}
