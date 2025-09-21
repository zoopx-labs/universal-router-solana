# Devcontainer Dockerfile for universal-router-solana
# Installs Rust (rustup), Solana CLI, and Anchor CLI (via cargo install) so anchor runs in-container.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Basic deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    llvm \
    clang \
    lld \
    cmake \
    git \
    wget \
    unzip \
    jq \
    zlib1g-dev \
    libudev-dev \
    libelf-dev \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Install rustup (non-interactive)
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=$CARGO_HOME/bin:$PATH
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
RUN rustup default stable
RUN rustup component add clippy rustfmt

# Install Solana CLI (stable channel)
RUN sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
ENV PATH=/root/.local/share/solana/install/active_release/bin:$PATH

# Install Anchor CLI via cargo so it's built for this container's glibc
RUN cargo install --locked anchor-cli

# Create workspace dir
WORKDIR /workspace

# non-root user not strictly required; keep root for simplicity in container

# Small health checks
RUN --mount=type=cache,target=/root/.cargo/registry \
    echo "anchor: $(anchor --version 2>/dev/null || echo 'not-installed')" || true

CMD ["bash"]
