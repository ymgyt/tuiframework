# List development commands.
default:
    @just --list

# Format all Rust sources.
fmt:
    cargo fmt --all

# Run the checks used in CI.
check: fmt-check lint test doc

fmt-check:
    cargo fmt --all --check

lint:
    cargo clippy --locked --all-targets -- -D warnings

test:
    cargo test --locked

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps

# Build and verify the package that would be uploaded to crates.io.
publish-dry-run:
    cargo publish --locked --registry crates-io --dry-run

# Publish to crates.io using CARGO_REGISTRY_TOKEN.
publish:
    cargo publish --locked --registry crates-io

# Run the example on a directory.
run directory=".":
    cargo run --locked --example files -- {{quote(directory)}}
