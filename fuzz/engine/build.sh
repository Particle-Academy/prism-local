#!/usr/bin/env bash
# Builds the coverage-guided fuzz engine. Run from the repository root.
# Instruments the parser crates and the fuzz library with SanitizerCoverage;
# the engine itself is compiled without coverage because it holds the hooks.
# PC_TABLE=1 adds the PC table needed by `engine ... lines` (Windows only).
set -euo pipefail
OUT=target/fuzz-cov
FLAGS="-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=3"
FLAGS="$FLAGS -C llvm-args=-sanitizer-coverage-trace-pc-guard"
FLAGS="$FLAGS -C llvm-args=-sanitizer-coverage-trace-compares"
if [ "${PC_TABLE:-0}" = 1 ]; then
  FLAGS="$FLAGS -C llvm-args=-sanitizer-coverage-pc-table"
fi
# opt-level 1: higher levels trip an LLVM COMDAT error with sancov on COFF.
FLAGS="$FLAGS -C opt-level=1 -C codegen-units=1 -C debug-assertions=on -C overflow-checks=on"
RUSTFLAGS="$FLAGS" cargo build -j 2 -p prism-local-fuzz --lib --target-dir "$OUT" --locked --offline
D="$OUT/debug"
# Cargo keeps each dependency's rlib in its own build/<crate>/<hash>/out
# directory, so search them all and take the newest core rlib.
SEARCH=(-L "dependency=$D")
while IFS= read -r dir; do SEARCH+=(-L "dependency=$dir"); done \
  < <(find "$D" -name '*.rlib' -exec dirname {} \; | sort -u)
CORE=$(ls -t $(find "$D" -name 'libprism_local_core*.rlib') | head -n 1)
EXE=""
if rustc --print cfg | grep -q 'target_os="windows"'; then EXE=.exe; fi
rustc --edition=2021 -C opt-level=2 -g "${SEARCH[@]}" \
  --extern "prism_local_fuzz=$D/libprism_local_fuzz.rlib" \
  --extern "prism_local_core=$CORE" \
  fuzz/engine/engine.rs -o "$OUT/engine$EXE"
echo "built $OUT/engine"
