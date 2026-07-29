#!/usr/bin/env bash
set -euo pipefail

# Run inside `nix develop`. This starts an isolated PipeWire daemon, creates one
# exact-name null sink, and runs the ignored Rust drain-completion smoke test.
runtime_dir=$(mktemp -d)
daemon_log="$runtime_dir/pipewire.log"
cleanup() {
  if [[ -n "${pipewire_pid:-}" ]]; then
    kill "$pipewire_pid" 2>/dev/null || true
    wait "$pipewire_pid" 2>/dev/null || true
  fi
  rm -rf "$runtime_dir"
}
trap cleanup EXIT
chmod 700 "$runtime_dir"
export XDG_RUNTIME_DIR="$runtime_dir"
export PIPEWIRE_RUNTIME_DIR="$runtime_dir"
export SOPHON_PIPEWIRE_SMOKE_NODE="sophon.test.sink"

pipewire >"$daemon_log" 2>&1 &
pipewire_pid=$!
for _ in $(seq 1 100); do
  if pw-cli info 0 >/dev/null 2>&1; then
    break
  fi
  sleep 0.05
done
pw-cli info 0 >/dev/null 2>&1 || {
  cat "$daemon_log" >&2
  exit 1
}

pw-cli create-node adapter \
  '{ factory.name=support.null-audio-sink node.name=sophon.test.sink media.class=Audio/Sink object.linger=true audio.position=[ MONO ] }' \
  >/dev/null

cargo test --lib playback::tests::pipewire_smoke_negotiates_and_drains -- --ignored --exact
