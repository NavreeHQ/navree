import NavreeProtocol
import Testing

@Test func theWebViewTokenNamesTheProtocolAndTheLibrary() {
    #expect(Wire.userAgentToken(platform: "ios", lib: "0.1.0") == "NavreeNative/1 (platform=ios; lib=0.1.0)")
    #expect(Wire.userAgentToken(platform: "ios; x", lib: "0.1 beta") == "NavreeNative/1 (platform=iosx; lib=0.1beta)")
    #expect(Wire.contentType == "application/vnd.navree.native+json")
}
