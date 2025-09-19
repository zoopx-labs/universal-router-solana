Title: Upgrade `ring` to 0.17.x (RUSTSEC-2025-0009 / RUSTSEC-2025-0010)

Summary:
Our dependency graph includes `ring 0.16.20`, which is flagged for a potential panic in some AES functions when overflow checking is enabled and is also marked unmaintained prior to 0.17.x.

Request:
Please consider upgrading `ring` to 0.17.x in the next release of the Solana/Anchor stacks, or advise on mitigations for downstream projects.

Details:
- Advisories: https://rustsec.org/advisories/RUSTSEC-2025-0009 and https://rustsec.org/advisories/RUSTSEC-2025-0010
- Affected package: ring 0.16.20
- Suggested fix: upgrade to 0.17.x

We can assist with PRs or compatibility testing.
