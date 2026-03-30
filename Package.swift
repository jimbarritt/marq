// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "marq",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "marq",
            exclude: ["Info.plist"],
            resources: [.copy("Resources")]
        )
    ]
)
