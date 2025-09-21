Devcontainer for universal-router-solana

Purpose
- Provides a reproducible Linux environment with Rust, Solana CLI, and Anchor CLI built from source (via cargo install) so `anchor build` and `solana program deploy` run cleanly without relying on host glibc.

Usage
1) In VS Code, install the Remote - Containers extension.
2) Open this repository in VS Code.
3) Command palette -> Remote-Containers: Reopen in Container. The container will build (may take 10-20 minutes on first build).
4) Once inside the container, run the smoke script (dry-run first):

   ./scripts/smoke-devnet.sh --keypair secure-keys/ZPXhajNNajSB4AYQMBqZajugyhDVqJPgD9CaRczBQMZ.json

   When you're ready to actually deploy:

   ./scripts/smoke-devnet.sh --keypair secure-keys/ZPXhajNNajSB4AYQMBqZajugyhDVqJPgD9CaRczBQMZ.json --no-dry-run

Notes
- The container builds `anchor` via `cargo install anchor-cli`. If you'd prefer a specific Anchor version you can edit the Dockerfile to pin a version.
- The container runs as root for simplicity. Adjust `remoteUser` in `devcontainer.json` if you prefer a non-root user.
- Do not commit private key files. Keep your deploy key in `secure-keys/` which is in `.gitignore`.
