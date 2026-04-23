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
ENV CARGO_INCREMENTAL=0

# Expose the API port
EXPOSE 3000

# Entrypoint
CMD ["coder-mcp"]
