import NavreeProtocol

/// How a message of a conversation says what it is and how far it got.
public enum MessageRules {
    /// What an internal note is called beside its symbol.
    public static let noteWords = "Internal Note"

    /// How far a message got, in words with a symbol, as Messages says it;
    /// nothing for one that is simply there.
    public static func delivery(_ delivery: Navree.Delivery) -> (text: String, symbol: String)? {
        switch delivery {
        case .sending: ("Sending…", "clock")
        case .sent: ("Sent", "checkmark")
        case .delivered: ("Delivered", "checkmark")
        case .read: ("Read", "checkmark.circle")
        case .failed: ("Not Delivered", "exclamationmark.circle.fill")
        }
    }
}
