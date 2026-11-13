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
            targets: ["NavreeProtocol", "NavreeDesign", "NavreeShell", "NavreeCoreBridge", "NavreeRuntime"]
        ),
        // The protocol and the drawing alone, without the core: what the
        // picture tool in Shots/ draws screens with.
        .library(name: "NavreeDesign", targets: ["NavreeProtocol", "NavreeDesign"]),
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

// The runtime core, written in Rust, through its C interface: the libraries
// that scripts/build-core.sh makes into NavreeCore.xcframework (never
// committed; run it before the first build), and the Swift class over them.
package.targets += [
    .binaryTarget(name: "NavreeCore", path: "NavreeCore.xcframework"),
    .target(
        name: "NavreeCoreBridge",
        dependencies: ["NavreeCore"],
        // What the libraries link beside them, as the compiler says it:
        // -lc -lm -liconv -lSystem, of which only iconv is not in libSystem.
        linkerSettings: [.linkedLibrary("iconv")]
    ),
    .testTarget(name: "NavreeCoreBridgeTests", dependencies: ["NavreeCoreBridge"]),
    // The loop over the core: its events, effects and view state as Swift
    // types, and the observed model the drawing reads.
    .target(name: "NavreeRuntime", dependencies: ["NavreeProtocol", "NavreeCoreBridge"]),
    .testTarget(name: "NavreeRuntimeTests", dependencies: ["NavreeRuntime", "NavreeProtocol", "NavreeCoreBridge"]),
]
