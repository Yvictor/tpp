# TPP - Token Pool Proxy

# Extract version from Cargo.toml
version := `grep '^version' Cargo.toml | head -1 | cut -d'"' -f2`
image := "sinotrade/tpp"

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

# Docker build with version and latest tags
image:
    @echo "Building Docker image {{image}}:{{version}} and {{image}}:latest"
    docker build --rm -t {{image}}:{{version}} -t {{image}}:latest .

# Docker build without cache
image-nocache:
    docker build --no-cache --rm -t {{image}}:{{version}} -t {{image}}:latest .

# Push both version and latest tags to Docker Hub
push: push-version push-latest
    @echo "Successfully pushed all tags"

# Push version tag to Docker Hub
push-version:
    @echo "Pushing {{image}}:{{version}}..."
    docker push {{image}}:{{version}}

# Push latest tag to Docker Hub
push-latest:
    @echo "Pushing {{image}}:latest..."
    docker push {{image}}:latest

# Build and push Docker image
image-push: image push

# Docker run
docker-run config="config.yaml":
    docker run -v $(pwd)/{{config}}:/app/config.yaml -p 8080:8080 -p 9090:9090 {{image}}:latest

# Show current version
show-version:
    @echo "Current version: {{version}}"
    @echo "Docker image: {{image}}:{{version}}"

# List local Docker images
list-images:
    @docker images {{image}} --format "table {{{{.Tag}}}}\t{{{{.Size}}}}\t{{{{.CreatedAt}}}}"

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
tag ver:
    git tag -a v{{ver}} -m "v{{ver}}"
    git push origin v{{ver}}
    @echo "Now create a release on GitHub: https://github.com/Yvictor/tpp/releases/new?tag=v{{ver}}"

# Show help
help:
    @just --list
