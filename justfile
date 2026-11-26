# List development commands.
[group("dev")]
default:
    @just --list

# Format all Rust sources.
[group("dev")]
fmt:
    cargo fmt --all

# Run the example on a directory.
[group("dev")]
run directory=".":
    cargo run --locked --example files -- {{quote(directory)}}

# Run the checks used in CI.
[group("check")]
check: fmt-check lint test doc

[group("check")]
fmt-check:
    cargo fmt --all --check

[group("check")]
lint:
    cargo clippy --locked --all-targets -- -D warnings

[group("check")]
test:
    cargo test --locked

[group("check")]
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps

# Preview a release with `just release patch`. Add `--execute` to commit, tag, and push.
[group("release")]
[positional-arguments]
release level *options:
    cargo release "$@"

# Build and verify the package that would be uploaded to crates.io.
[group("release")]
publish-dry-run:
    cargo publish --locked --registry crates-io --dry-run

# Publish to crates.io using CARGO_REGISTRY_TOKEN.
[group("release")]
publish:
    cargo publish --locked --registry crates-io
