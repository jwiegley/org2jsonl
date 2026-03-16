#!/usr/bin/env bash
# Check for benchmark regressions against a saved baseline.
# If no baseline exists, create one and exit successfully.
set -euo pipefail

THRESHOLD=5
BASELINE_NAME="baseline"

# Check if a baseline exists (criterion saves data under target/criterion/)
if [ ! -d "target/criterion" ] || [ -z "$(ls -A target/criterion 2>/dev/null)" ]; then
    echo "No benchmark baseline found. Running initial benchmarks..."
    cargo bench --bench roundtrip -- --save-baseline "$BASELINE_NAME"
    echo "Baseline saved. Future commits will be compared against this."
    exit 0
fi

# Run current benchmarks
cargo bench --bench roundtrip -- --save-baseline current

# Compare against baseline
if command -v critcmp &>/dev/null; then
    echo "Comparing against baseline (threshold: ${THRESHOLD}%)..."
    critcmp "$BASELINE_NAME" current --threshold "$THRESHOLD"
else
    echo "Warning: critcmp not found; skipping regression comparison"
    echo "Install with: cargo install critcmp"
fi
