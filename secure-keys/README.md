secure-keys — local operator key storage (do not commit)

This folder is intended as a local, gitignored place for operators to keep JSON keypairs used for deploying and managing programs during development or on Devnet.

Guidelines
- Never commit private keys to the repository.
- Keep the directory in your personal, secure machine (or a secrets manager). The repo `.gitignore` already ignores `secure-keys/*`.
- Use filenames that make intent clear, for example: `devnet-deployer.json`, `devnet-upgrade-authority.json`.

Example usage in scripts
- export PROGRAM_KEYPAIR_PATH=/absolute/path/to/secure-keys/devnet-deployer.json
- scripts/deploy-devnet.sh will read `$PROGRAM_KEYPAIR_PATH` when present.

If you want me to wire a specific path into `Anchor.toml` or `.env` for local docs, provide the filename (I will not add the file to the repo).