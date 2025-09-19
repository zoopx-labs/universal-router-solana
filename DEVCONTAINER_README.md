# Dev Container for universal-router-solana

This folder contains a minimal VS Code Dev Container configuration that provides a reproducible
environment for building and running the `zpx_router` tests (including the Keccak-based golden
vector generator).

How to use
1. Install Docker and the VS Code Dev Containers extension.
2. In VS Code, choose: `Dev Containers: Open Folder in Container...` and select this repository.
3. The container will build using `.devcontainer/Dockerfile`. On first open the `postCreateCommand`
   will regenerate golden vectors and run `cargo build -p zpx_router`.

Run tests inside the container
- Unit tests (fast):
  ```bash
  cargo test -p zpx_router
  ```

- Program-tests (requires Solana test harness):
  ```bash
  # run integration tests (ignored by default)
  cargo test -p zpx_router --features program-test -- --ignored
  ```

Notes
- The container installs Solana CLI v1.18.26 and Python keccak libraries (`pycryptodome` and `pysha3`).
- If you need to pin other toolchain versions, edit `.devcontainer/Dockerfile` accordingly.
