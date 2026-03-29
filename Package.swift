// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "marq",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "marq",
            resources: [.copy("Resources")]
        )
    ]
)
