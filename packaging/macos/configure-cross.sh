#!/usr/bin/env bash
set -euo pipefail

target="${1:?Rust target is required}"
case "$target" in
  aarch64-apple-darwin) clang_target=arm64-apple-macos14.0 ;;
  x86_64-apple-darwin) clang_target=x86_64-apple-macos14.0 ;;
  *) echo "unsupported macOS target: $target" >&2; exit 2 ;;
esac
: "${SDKROOT:?Apple SDK is required}"
: "${LLVM_ROOT:?LLVM installation is required}"
test -d "$SDKROOT/System/Library/Frameworks"
for tool in clang ld64.lld llvm-ar; do
  test -x "$LLVM_ROOT/bin/$tool"
done

# This driver serves both cc-rs Objective-C compilation and Rust's final link.
# Keep target flags out of host build scripts and proc macros.
wrapper="$RUNNER_TEMP/keysteer-$target-clang"
{
  echo '#!/usr/bin/env bash'
  printf 'exec %q --target=%q -isysroot %q -mmacosx-version-min=14.0 -fuse-ld=lld "$@"\n' \
    "$LLVM_ROOT/bin/clang" "$clang_target" "$SDKROOT"
} > "$wrapper"
chmod +x "$wrapper"
target_env="${target//-/_}"
{
  echo "CC_${target_env}=$wrapper"
  echo "AR_${target_env}=$LLVM_ROOT/bin/llvm-ar"
  echo "CARGO_TARGET_${target_env^^}_LINKER=$wrapper"
} >> "$GITHUB_ENV"
