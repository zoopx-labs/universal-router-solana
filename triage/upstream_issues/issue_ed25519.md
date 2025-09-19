Title: Upgrade `ed25519-dalek` to v2.x to address advisory RUSTSEC-2022-0093

Summary:
Our dependency graph includes `ed25519-dalek 1.0.1`, which is flagged by RUSTSEC-2022-0093 (Double Public Key Signing Function Oracle Attack). This is transitively introduced via `solana-sdk`.

Request:
Please consider upgrading to `ed25519-dalek >=2.0` (which provides safer APIs) or applying mitigations. We can open a PR or provide testcases if it helps.

Details:
- Advisory: https://rustsec.org/advisories/RUSTSEC-2022-0093
- Affected package: ed25519-dalek 1.0.1
- Suggested fix: upgrade to >=2.0

Thanks.
