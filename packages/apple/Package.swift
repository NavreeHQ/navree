// swift-tools-version: 6.2

// The Apple client of Navree Native: the protocol's types, the system design
// layer that places and draws each part as Apple's guidelines say, and the
// shell that holds the app's destinations, its web view and its session.
import PackageDescription

let package = Package(
    name: "NavreeNative",
    platforms: [
        .iOS(.v26),
        .macOS(.v26),
    ],
    products: [
        .library(
            name: "NavreeNative",
            targets: ["NavreeProtocol", "NavreeDesign", "NavreeShell"]
        )
    ],
    targets: [
        .target(name: "NavreeProtocol"),
        .target(name: "NavreeDesign", dependencies: ["NavreeProtocol"]),
        .target(name: "NavreeShell", dependencies: ["NavreeProtocol", "NavreeDesign"]),
        .testTarget(name: "NavreeProtocolTests", dependencies: ["NavreeProtocol"]),
        .testTarget(name: "NavreeDesignTests", dependencies: ["NavreeDesign", "NavreeProtocol"]),
        .testTarget(name: "NavreeShellTests", dependencies: ["NavreeShell", "NavreeProtocol"]),
    ]
)
