#!/usr/bin/env bash
set -euo pipefail

CRATE_NAME="mdict_tools"
OUTPUT_DIR="output_framework"
XCFRAMEWORK_DIR="$OUTPUT_DIR/$CRATE_NAME.xcframework"
SWIFT_PKG_DIR="$OUTPUT_DIR/SwiftPackage"

echo "Cleaning old outputs..."
rm -rf "$OUTPUT_DIR"

echo "Ensuring Rust targets..."
rustup target add aarch64-apple-ios
rustup target add aarch64-apple-ios-sim

####################################
# 1. Build static libraries
####################################
echo "Building static libs for iOS..."

export IPHONEOS_DEPLOYMENT_TARGET=16.0
# iOS devices
cargo build --release --target aarch64-apple-ios
# iOS simulator (Apple Silicon)
cargo build --release --target aarch64-apple-ios-sim

####################################
# 2. Create universal static lib (iOS only)
####################################
echo "Making universal iOS library..."

IOS_LIBS=()

for ARCH in aarch64-apple-ios x86_64-apple-ios
do
    IOS_LIBS+=("target/$ARCH/release/lib$CRATE_NAME.a")
done

# Make combined static lib for iOS
mkdir -p "target/universal-ios"

####################################
# 3. Generate Swift bindings
####################################
echo "Generating Swift bindings using UniFFI..."
# Build a cdylib for host platform to genera

mkdir -p "$OUTPUT_DIR/swift"
cargo build --release
cargo run --bin uniffi-bindgen generate \
    --library "target/release/lib$CRATE_NAME.dylib" \
    --language swift \
    --out-dir "$OUTPUT_DIR/swift"

####################################
# 4. Build headers & module map
####################################
echo "Copying headers..."

mkdir -p "$OUTPUT_DIR/headers"
cp "$OUTPUT_DIR/swift/"*.h "$OUTPUT_DIR/headers/"

# Generate module.modulemap in headers for XCFramework
cat > "$OUTPUT_DIR/headers/module.modulemap" << EOF
module ${CRATE_NAME}FFI {
    header "${CRATE_NAME}FFI.h"
    export *
}
EOF

####################################
# 5. Create XCFramework (iOS only)
####################################
echo "Creating XCFramework..."

mkdir -p "$XCFRAMEWORK_DIR"

xcodebuild -create-xcframework \
    -library "target/aarch64-apple-ios/release/lib$CRATE_NAME.a" \
    -headers "$OUTPUT_DIR/headers" \
    -library "target/aarch64-apple-ios-sim/release/lib$CRATE_NAME.a" \
    -headers "$OUTPUT_DIR/headers" \
    -output "$XCFRAMEWORK_DIR"

####################################
# 6. Create Swift Package
####################################
echo "Creating Swift Package..."

mkdir -p "$SWIFT_PKG_DIR/Sources/$CRATE_NAME"

# Copy Swift source files and headers to the package
echo "Copying Swift source files and headers..."
cp "$OUTPUT_DIR/swift/"*.swift "$SWIFT_PKG_DIR/Sources/$CRATE_NAME/"

cat > "$SWIFT_PKG_DIR/Package.swift" << EOF
// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "$CRATE_NAME",
    platforms: [
        .iOS(.v13)
    ],
    products: [
        .library(
            name: "$CRATE_NAME",
            targets: ["$CRATE_NAME"]
        )
    ],
    targets: [
        .target(
            name: "$CRATE_NAME",
            dependencies: ["${CRATE_NAME}FFI"],
            path: "Sources/$CRATE_NAME",
        ),
        .binaryTarget(
            name: "${CRATE_NAME}FFI",
            path: "$CRATE_NAME.xcframework"
        )
    ]
)
EOF

echo "Copying XCFramework into Swift package..."
cp -R "$XCFRAMEWORK_DIR" "$SWIFT_PKG_DIR/$CRATE_NAME.xcframework"

echo "Done. Output in $OUTPUT_DIR"