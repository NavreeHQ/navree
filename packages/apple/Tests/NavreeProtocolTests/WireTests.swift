import NavreeProtocol
import Testing

@Test func theWebViewTokenNamesTheProtocolMajor() {
    #expect(Wire.userAgentToken == "NavreeNative/1")
    #expect(Wire.contentType == "application/vnd.navree.native+json")
}
