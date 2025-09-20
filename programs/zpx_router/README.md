# zpx_router

This program is developed with Anchor (Anchor.toml present). Use the Anchor CLI to build the IDL and SBF artifacts for deployment.

When iterating locally you can use `cargo test -p zpx_router --lib` to run unit tests. For producing deployable artifacts and the Anchor IDL, use `anchor build`.

CI is configured in `.github/workflows/anchor-build.yml` to install a pinned Solana CLI and install Anchor (via cargo) and run `anchor build`. This avoids relying on a preinstalled Anchor binary present in the environment.
