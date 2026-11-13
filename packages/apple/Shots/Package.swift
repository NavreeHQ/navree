// swift-tools-version: 6.2

// Pictures of screens from recorded answers, drawn by the client's own views
// in a window of their own: built for the Mac as an iPad app (Mac Catalyst),
// so the system's lists, bars and controls draw as they do on iOS, with no
// simulator. scripts/shots.sh builds it, wraps it as an app and runs it.
import PackageDescription

let package = Package(
    name: "NavreeShots",
    platforms: [.iOS(.v26), .macOS(.v26)],
    dependencies: [.package(name: "NavreeNative", path: "..")],
    targets: [
        .executableTarget(
            name: "NavreeShots",
            dependencies: [.product(name: "NavreeDesign", package: "NavreeNative")]
        )
    ]
)
