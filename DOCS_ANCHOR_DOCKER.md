Anchor Docker HOWTO

This repository includes a reproducible Docker image to run Anchor builds and tests without relying on the host's glibc or preinstalled Anchor binary.

Build the image:

```bash
# from repo root
docker build -f Dockerfile.anchor-build -t anchor-build:0.31 .
```

Run an interactive container with your workspace mounted:

```bash
docker run --rm -it -v $(pwd):/workspace -w /workspace anchor-build:0.31
# inside the container
anchor --version
anchor build
```

Or run a single command (CI friendly):

```bash
docker run --rm -v $(pwd):/workspace -w /workspace anchor-build:0.31 anchor build
```

Notes:
- The image pins Solana CLI v1.14.16 and Anchor v0.31.1 (via cargo install), matching the CI workflow.
- Building the image will take some time the first run (compiling Anchor from source). Use CI caching in GitHub Actions for speedups.
- For VS Code Remote Containers, a `.devcontainer/Dockerfile.anchor-build` is included.
