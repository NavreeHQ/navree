/// What a client says about itself on every request, in ``Wire/clientHeader``:
/// `platform=ios; os=26.1; form=phone; app=1.4.0; lib=0.1.0; vocab=1; ...`.
///
/// The server lowers a page from its tree and the vocabulary only; the rest is
/// for handlers that ask, and for the coverage report. It never decides where
/// a part goes: that is the client's design layer, from the window's width.
public struct ClientDescriptor: Sendable, Equatable {
    public var platform: String
    public var os: String
    public var form: String
    public var app: String
    /// The version of this client library.
    public var lib: String
    public var vocab: Int
    public var scheme: String?
    public var contrast: String?
    /// The reader's text size, as a factor of the default.
    public var text: Double?
    public var locale: String?
    public var timeZone: String?
    /// The device features this build of the app may use.
    public var caps: [String]

    public init(
        platform: String, os: String, form: String, app: String, lib: String,
        vocab: Int = Wire.vocab, scheme: String? = nil, contrast: String? = nil, text: Double? = nil,
        locale: String? = nil, timeZone: String? = nil, caps: [String] = []
    ) {
        self.platform = platform
        self.os = os
        self.form = form
        self.app = app
        self.lib = lib
        self.vocab = vocab
        self.scheme = scheme
        self.contrast = contrast
        self.text = text
        self.locale = locale
        self.timeZone = timeZone
        self.caps = caps
    }

    /// The header's value: `key=value` pairs, each value a token.
    public var header: String {
        var pairs: [(String, String)] = [
            ("platform", platform), ("os", os), ("form", form), ("app", app), ("lib", lib), ("vocab", String(vocab)),
        ]
        if let scheme { pairs.append(("scheme", scheme)) }
        if let contrast { pairs.append(("contrast", contrast)) }
        if let text { pairs.append(("text", Self.number(text))) }
        if let locale { pairs.append(("locale", locale)) }
        if let timeZone { pairs.append(("tz", timeZone)) }
        pairs.append(("caps", caps.map(Self.token).joined(separator: "+")))
        return pairs.map { "\($0.0)=\(Self.token($0.1))" }.joined(separator: "; ")
    }

    /// A value made safe for the header: no separators, no spaces.
    static func token(_ value: String) -> String {
        String(value.unicodeScalars.filter { scalar in
            scalar.isASCII && scalar.value > 32 && scalar != ";" && scalar != "=" && scalar != ","
        }.map(Character.init))
    }

    static func number(_ value: Double) -> String {
        let rounded = (value * 100).rounded() / 100
        return rounded == rounded.rounded() ? String(Int(rounded)) + ".0" : String(rounded)
    }
}
