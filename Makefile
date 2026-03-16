# Coverage configuration
COVERAGE_THRESHOLD ?= 89
IGNORE_REGEX = tests/.*|benches/.*|examples/.*|bin/.*

.PHONY: build test fmt lint doc coverage coverage-html coverage-lcov \
        coverage-check bench bench-baseline bench-check fuzz clean

# Build release binaries
build:
	cargo build --release

# Run all tests
test:
	cargo test

# Check formatting
fmt:
	cargo fmt -- --check

# Run clippy with warnings-as-errors
lint:
	cargo clippy --all-targets -- -D warnings

# Build documentation (warnings are errors)
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Run tests with coverage and show summary
coverage:
	cargo llvm-cov test --ignore-filename-regex "$(IGNORE_REGEX)"

# Generate HTML coverage report and open in browser
coverage-html:
	cargo llvm-cov test --ignore-filename-regex "$(IGNORE_REGEX)" --html --open

# Generate LCOV coverage report
coverage-lcov:
	cargo llvm-cov test --ignore-filename-regex "$(IGNORE_REGEX)" --lcov --output-path lcov.info

# Run coverage and fail if below threshold
coverage-check:
	cargo llvm-cov test --ignore-filename-regex "$(IGNORE_REGEX)" --fail-under-lines $(COVERAGE_THRESHOLD)

# Run benchmarks
bench:
	cargo bench --bench roundtrip

# Save a benchmark baseline for future regression comparison
bench-baseline:
	cargo bench --bench roundtrip -- --save-baseline baseline

# Check for benchmark regressions against saved baseline
bench-check:
	scripts/bench-check.sh

# Run fuzz testing (requires nightly: rustup run nightly cargo fuzz)
fuzz:
	cd fuzz && cargo +nightly fuzz run fuzz_parse -- -max_total_time=60
	cd fuzz && cargo +nightly fuzz run fuzz_roundtrip -- -max_total_time=60

clean:
	cargo clean
