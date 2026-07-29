#!/usr/bin/env bash
set -euo pipefail

crate_manifest="$(cd "$(dirname "$0")/.." && pwd)/Cargo.toml"

expect_failure() {
  local label=$1
  local expected=$2
  shift 2
  local output
  output=$(mktemp)
  if cargo check --manifest-path "$crate_manifest" "$@" >"$output" 2>&1; then
    echo "expected $label configuration to fail" >&2
    rm -f "$output"
    return 1
  fi
  if ! grep -F "$expected" "$output" >/dev/null; then
    cat "$output" >&2
    echo "$label failure did not contain expected diagnostic: $expected" >&2
    rm -f "$output"
    return 1
  fi
  rm -f "$output"
}

expect_failure "zero-backend" "requires exactly one acceleration feature" --no-default-features
expect_failure "conflicting-backend" "these were enabled together: \`cpu\`, \`cuda\`" --features cuda

for backend in cuda sycl vulkan; do
  output=$(mktemp)
  if ! cargo check --manifest-path "$crate_manifest" --no-default-features --features "$backend" >"$output" 2>&1; then
    grep -F "qwentts-cpp \`$backend\` backend prerequisite is unavailable" "$output" >/dev/null || {
      cat "$output" >&2
      echo "$backend configuration failure did not identify its selected backend prerequisites" >&2
      rm -f "$output"
      exit 1
    }
  fi
  rm -f "$output"
done
