/// The names and numbers a client and a Navree server agree on.
public enum Wire {
    /// The protocol's major version: the value of ``nativeHeader`` and of an
    /// envelope's `navree`. Each side refuses a major it does not know.
    public static let major = 1

    /// The vocabulary this client draws: the number it sends as `vocab`. It
    /// stays 0 until the vocabulary is frozen.
    public static let vocab = 0

    /// The request header that says "answer me as a native client".
    public static let nativeHeader = "X-Navree-Native"

    /// The request header that carries the client's descriptor.
    public static let clientHeader = "X-Navree-Client"

    /// The media type of an envelope and of the shell document.
    public static let contentType = "application/vnd.navree.native+json"

    /// Where the app's shell document is.
    public static let shellPath = "/_navree/native/shell"

    /// The product a client's web view adds to its user agent, so the server
    /// writes a page without the chrome the client draws itself. A web view
    /// sends custom headers on its first request only, so a header can't say it.
    public static let userAgentProduct = "NavreeNative/\(major)"

    /// The whole token: `NavreeNative/1 (platform=ios; lib=0.1.0)`.
    public static func userAgentToken(platform: String, lib: String) -> String {
        "\(userAgentProduct) (platform=\(ClientDescriptor.token(platform)); lib=\(ClientDescriptor.token(lib)))"
    }
}
