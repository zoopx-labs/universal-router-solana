Title: Please upgrade `borsh` to a patched version (RUSTSEC-2023-0033)

Summary:
Our repository's dependency graph includes `borsh 0.9.3` which is reported as unsound when parsing ZSTs (RUSTSEC-2023-0033). This is transitively pulled in via `solana-program` / `anchor-lang` in our workspace.

Request:
Could the Solana/Anchor maintainers consider upgrading `borsh` to `^0.10.4` or a patched 1.0+ release in the next release cycle? This would mitigate the unsoundness advisory flagged by cargo-deny.

Details:
- Advisory: https://rustsec.org/advisories/RUSTSEC-2023-0033
- Affected package: borsh 0.9.3
- Suggested fix: upgrade to ^0.10.4 or >=1.0.0-alpha.1

Thanks — we can open a PR to help if maintainers want assistance.
