# Coverage configuration
COVERAGE_THRESHOLD ?= 90
IGNORE_REGEX = tests/.*|benches/.*|examples/.*

.PHONY: coverage coverage-html coverage-lcov coverage-check test build clean

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

# Standard targets
test:
	cargo test

build:
	cargo build --release

clean:
	cargo clean
