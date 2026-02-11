# Build stage
FROM rust:1.92-slim-bookworm AS builder

# Install python3 for pyo3
RUN apt-get update && apt-get install -y python3 && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app
COPY . .

# Build the server binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Set environment for Rust
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# Install runtime dependencies, build tools, and minimal Rust toolchain
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    curl \
    wget \
    git \
    build-essential \
    pkg-config \
    libssl-dev \
    ca-certificates \
    mold \
    clang \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.92 --profile minimal \
    && rm -rf /var/lib/apt/lists/*

# Install sccache
RUN arch=$(uname -m) && \
    if [ "$arch" = "x86_64" ]; then \
        url="https://github.com/mozilla/sccache/releases/download/v0.12.0/sccache-v0.12.0-x86_64-unknown-linux-musl.tar.gz"; \
    elif [ "$arch" = "aarch64" ]; then \
        url="https://github.com/mozilla/sccache/releases/download/v0.12.0/sccache-v0.12.0-aarch64-unknown-linux-musl.tar.gz"; \
    fi && \
    wget -q $url -O sccache.tar.gz \
    && tar xzf sccache.tar.gz \
    && mv sccache-v0.12.0-*/sccache /usr/local/bin/sccache \
    && chmod +x /usr/local/bin/sccache \
    && rm -rf sccache.tar.gz sccache-v0.12.0-*

# Setup sccache directory
RUN mkdir -p /var/cache/sccache \
    && chmod 777 /var/cache/sccache

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/coder-mcp /usr/local/bin/

# Create a workspace directory for the agent
RUN mkdir -p /workspace
WORKDIR /workspace

# Configure cargo to use mold with gcc
RUN mkdir -p /root/.cargo && \
    echo '[target.x86_64-unknown-linux-gnu]\nrustflags = ["-C", "link-arg=-fuse-ld=mold"]\n\n[target.aarch64-unknown-linux-gnu]\nrustflags = ["-C", "link-arg=-fuse-ld=mold"]' > /root/.cargo/config.toml

# Set environment variables
ENV RUST_LOG=info
ENV PORT=3000
ENV WORKSPACE_DIR=/workspace
ENV RUSTC_WRAPPER=/usr/local/bin/sccache
ENV SCCACHE_DIR=/var/cache/sccache
ENV CARGO_INCREMENTAL=0

# Expose the API port
EXPOSE 3000

# Entrypoint
CMD ["coder-mcp"]
