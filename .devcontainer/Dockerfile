FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive

# Basic toolchain
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    build-essential \
    curl \
    git \
    pkg-config \
    libssl-dev \
    clang \
    llvm \
    python3 \
    python3-pip \
    ca-certificates \
    unzip \
  && rm -rf /var/lib/apt/lists/*

# Install Rust (rustup) non-interactively
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="$CARGO_HOME/bin:$PATH"
RUN rustup default stable && rustup component add rustfmt clippy

# Install Solana CLI (pinned)
ENV SOLANA_VERSION=v1.18.26
RUN curl -sSfL https://release.solana.com/${SOLANA_VERSION}/install | sh
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"

# Python deps for golden vector generation
RUN python3 -m pip install --upgrade pip
RUN python3 -m pip install pycryptodome pysha3

# Make a workspace mount point
WORKDIR /workspace

# Default user is root in the container; VS Code will mount your workspace at /workspace

CMD ["/bin/bash"]
