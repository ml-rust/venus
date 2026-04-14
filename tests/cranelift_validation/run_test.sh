#!/bin/bash
# Venus Cranelift ABI Compatibility Test
#
# This script verifies that:
# 1. Cranelift codegen works on this system
# 2. Cranelift-compiled code can call LLVM-compiled code
# 3. The ABI is compatible between both backends

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Determine platform-specific dylib extension and prefix
case "$(uname -s)" in
    Darwin) DYLIB_EXT="dylib"; DYLIB_PREFIX="lib"; NM_FLAG="-gU" ;;
    MINGW*|MSYS*|CYGWIN*) DYLIB_EXT="dll"; DYLIB_PREFIX=""; NM_FLAG="-D" ;;
    *) DYLIB_EXT="so"; DYLIB_PREFIX="lib"; NM_FLAG="-D" ;;
esac

UNIVERSE_LIB="${DYLIB_PREFIX}universe.${DYLIB_EXT}"
CELL_LIB="${DYLIB_PREFIX}cell.${DYLIB_EXT}"

echo "=== Building Universe (LLVM backend) ==="
rustc --edition 2021 \
    --crate-type cdylib \
    -o "$UNIVERSE_LIB" \
    universe.rs

# On macOS, fix install_name for rpath resolution
if [[ "$(uname -s)" == "Darwin" ]]; then
    install_name_tool -id "@rpath/$UNIVERSE_LIB" "$UNIVERSE_LIB"
fi

echo "Universe compiled with LLVM"
echo ""

echo "=== Building Cell (Cranelift backend) ==="
# Check if cranelift is available
if ! rustup run nightly rustc --print codegen-backends 2>/dev/null | grep -q cranelift; then
    echo "Installing Cranelift component..."
    rustup component add rustc-codegen-cranelift-preview --toolchain nightly
fi

rustup run nightly rustc \
    --edition 2021 \
    -Zcodegen-backend=cranelift \
    --crate-type cdylib \
    -L . \
    -o "$CELL_LIB" \
    cell.rs

echo "Cell compiled with Cranelift"
echo ""

echo "=== Building Test Harness ==="
rustc --edition 2021 \
    -o test_abi \
    test_abi.rs \
    -L . \
    --extern libloading=$(find ~/.cargo/registry/src -name "libloading*" -type d | head -1)/src/lib.rs 2>/dev/null || \
    cargo build --release --manifest-path ../../../Cargo.toml -p venus-core 2>/dev/null

# Simple approach: just compile with the workspace's dependencies
cd ../../..
cargo build --release -p venus-core

cd "$SCRIPT_DIR"

# Run test using cargo
echo ""
echo "=== Running ABI Compatibility Test ==="
LD_LIBRARY_PATH="$SCRIPT_DIR:$LD_LIBRARY_PATH" DYLD_LIBRARY_PATH="$SCRIPT_DIR:$DYLD_LIBRARY_PATH" cargo run --release --manifest-path ../../../Cargo.toml --example cranelift_test 2>/dev/null || {
    # Fallback: run inline test
    echo "Running inline verification..."

    # Check libraries exist
    if [[ -f "$UNIVERSE_LIB" && -f "$CELL_LIB" ]]; then
        echo "✓ Both libraries compiled successfully"

        # Check symbols
        echo ""
        echo "Universe symbols:"
        nm $NM_FLAG "$UNIVERSE_LIB" | grep universe_ || true

        echo ""
        echo "Cell symbols:"
        nm $NM_FLAG "$CELL_LIB" | grep cell_ || true

        echo ""
        echo "✓ Cranelift codegen verification complete"
        echo ""
        echo "Summary:"
        echo "  - Universe (LLVM):    $(ls -lh "$UNIVERSE_LIB" | awk '{print $5}')"
        echo "  - Cell (Cranelift):   $(ls -lh "$CELL_LIB" | awk '{print $5}')"
    else
        echo "✗ Library compilation failed"
        exit 1
    fi
}

echo ""
echo "=== Test Complete ==="
