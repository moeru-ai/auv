// swift-tools-version: 5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "AuvMacosNative",
    platforms: [
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "AuvMacosNative",
            type: .static,
            targets: ["AuvMacosNative"]
        ),
    ],
    targets: [
        .target(
            name: "AuvMacosNative",
            swiftSettings: [
                .unsafeFlags([
                    "-import-objc-header",
                    // NOTICE: SwiftPM invokes swiftc from `native/`, not this manifest's directory.
                    "swift/Sources/AuvMacosNative/Generated/native-bridging-header.h"
                ])
            ]
        ),
    ]
)
