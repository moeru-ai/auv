// swift-tools-version: 5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "AuvCaptureNative",
    platforms: [
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "AuvCaptureNative",
            type: .static,
            targets: ["AuvCaptureNative"]
        ),
    ],
    targets: [
        .target(
            name: "AuvCaptureNative",
            swiftSettings: [
                .unsafeFlags([
                    "-import-objc-header",
                    // NOTICE: The generated bridge is ignored by git and must be created with
                    // `scripts/generate-swift-bridge` before SwiftPM or SourceKit validation.
                    "Sources/AuvCaptureNative/Generated/native-bridging-header.h"
                ])
            ]
        ),
    ]
)
