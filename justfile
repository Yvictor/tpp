# TPP - Token Pool Proxy

# Default recipe
default: check

# Run all checks (fmt, clippy, test)
check: fmt clippy test

# Format code
fmt:
    cargo fmt --all

# Check format without modifying
fmt-check:
    cargo fmt --all -- --check

# Run clippy
clippy:
    cargo clippy --all-features -- -D warnings

# Run tests
test:
    cargo test --all-features

# Build debug
build:
    cargo build

# Build release
release:
    cargo build --release

# Run with config file
run config="config.yaml":
    cargo run -- --config {{config}}

# Run release with config file
run-release config="config.yaml":
    ./target/release/tpp --config {{config}}

# Clean build artifacts
clean:
    cargo clean

# Docker build
docker-build tag="tpp:latest":
    docker build -t {{tag}} .

# Docker run
docker-run tag="tpp:latest" config="config.yaml":
    docker run -v $(pwd)/{{config}}:/app/config.yaml -p 8080:8080 -p 9090:9090 {{tag}}

# Watch and run tests on file change (requires cargo-watch)
watch:
    cargo watch -x test

# Generate and open docs
docs:
    cargo doc --open

# Check for outdated dependencies (requires cargo-outdated)
outdated:
    cargo outdated

# Update dependencies
update:
    cargo update

# Create a new release tag
tag version:
    git tag v{{version}}
    git push origin v{{version}}
    @echo "Now create a release on GitHub: https://github.com/Yvictor/tpp/releases/new?tag=v{{version}}"

# Show help
help:
    @just --list
