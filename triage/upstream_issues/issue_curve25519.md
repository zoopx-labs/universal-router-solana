Title: Upgrade `curve25519-dalek` to mitigate timing-variability advisory (RUSTSEC-2024-0344)

Summary:
Our dependency graph includes `curve25519-dalek 3.2.1` which is flagged for timing variability in `Scalar29::sub`/`Scalar52::sub` (RUSTSEC-2024-0344). This is transitively pulled in via `ed25519-dalek`/`solana-sdk`.

Request:
Could maintainers upgrade `curve25519-dalek` to >=4.1.3 (or apply the backported fix) in the next release? We can assist with a PR or testing if helpful.

Details:
- Advisory: https://rustsec.org/advisories/RUSTSEC-2024-0344
- Affected package: curve25519-dalek 3.2.1
- Suggested fix: upgrade to >=4.1.3

Thank you.
