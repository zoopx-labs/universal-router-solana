# Security Policy

If you believe you've found a security vulnerability, please report it responsibly.

- Preferred contact: security@zoopx.example (PGP available on request)
- Please include a proof-of-concept and the impacted files (paths under programs/zpx_router/**).
- We aim to acknowledge within 3 business days and provide a timeline for a fix.
- Do not disclose publicly until a fix is available and coordinated.

PGP key: available on request from the security contact; do NOT paste private keys here.

If you need an encrypted channel to share sensitive POC files, ask the security contact for a secure upload location.

## Supply-chain checks (cargo-deny)

This repository includes a conservative `deny.toml` policy at the repository root. It is used to fail CI on problematic dependency findings (yanked crates, unmaintained crates, or disallowed licenses).

To run locally:

```bash
# install cargo-deny (recommended pinned version in CI)
cargo install cargo-deny --locked

# run the checks
cargo deny check -c deny.toml
```

When running in CI the `cargo-deny` job should be pinned to a known version and set to fail the build on any `deny`-level findings. If you need to allow a particular crate, add an explicit exception in `deny.toml` with a short justification and an owner.

## Recent supply-chain triage

We ran `cargo-deny` and produced normalized triage artifacts under `triage/`:

- `triage/cargo-deny-triage.json` — normalized diagnostic JSON exported from cargo-deny
- `triage/cargo-deny-classification.json` — per-advisory classification (fix-locally / upstream-required / accept-exception)
- `triage/deny-suggestions.toml` — suggested deny exceptions for low-risk dev-time crates
- `triage/cargo-update-cmds.txt` — suggested local updates and upstream-PR actions

Top-priority transitive advisories include: `borsh` (unsound), `ed25519-dalek` (vulnerability), `curve25519-dalek` (timing vulnerability), and `ring` (vulnerability/unmaintained). See `triage/upstream_issues/` for drafted issue templates to open against upstream maintainers.

If you accept temporary exceptions, add them to `deny.toml` with an owner and an expiry date (we include suggested entries in `deny-exceptions/deny-suggestions.toml`).
