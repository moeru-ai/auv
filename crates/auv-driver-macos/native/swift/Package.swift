// swift-tools-version: 6.0
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
        // Pure, Accessibility-free AX path parse layer.
        //
        // Split into its own target so `swift test` can cover it: the rest of
        // `AuvMacosNative` compiles `Generated/SwiftBridgeCore.swift`, whose Rust
        // FFI symbols only exist in the cargo-built static library, so a test
        // target that links it fails at `ld`. This target links nothing.
        //
        // NOTICE: `path:` + `sources:` keeps both targets in one source directory,
        // because `crates/auv-driver-macos/build.rs` globs
        // `Sources/AuvMacosNative/*.swift` (non-recursive) into a single flat
        // `swiftc -module-name AuvMacosNative` invocation. Moving this file out of
        // that directory would drop it from the shipped static library.
        .target(
            name: "AuvAxPath",
            path: "Sources/AuvMacosNative",
            sources: ["AxPath.swift"]
        ),
        .target(
            name: "AuvMacosNative",
            dependencies: ["AuvAxPath"],
            path: "Sources/AuvMacosNative",
            exclude: ["AxPath.swift"],
            swiftSettings: [
                .unsafeFlags([
                    "-import-objc-header",
                    // NOTICE: SwiftPM invokes swiftc from `native/`, not this manifest's directory.
                    "swift/Sources/AuvMacosNative/Generated/native-bridging-header.h"
                ])
            ]
        ),
        .testTarget(
            name: "AuvAxPathTests",
            dependencies: ["AuvAxPath"]
        ),
    ]
)
