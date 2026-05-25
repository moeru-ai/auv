// swift-tools-version: 5.5

import PackageDescription

let package = Package(
    name: "AuvMacosOverlayNative",
    platforms: [
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "AuvMacosOverlayNative",
            type: .static,
            targets: ["AuvMacosOverlayNative"]
        ),
    ],
    targets: [
        .target(
            name: "AuvMacosOverlayNative",
            swiftSettings: [
                .unsafeFlags([
                    "-import-objc-header",
                    "Sources/AuvMacosOverlayNative/Generated/native-bridging-header.h"
                ])
            ]
        ),
    ]
)
