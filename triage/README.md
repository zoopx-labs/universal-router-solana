This folder contains artifacts from cargo-deny triage runs and suggested remediation.

Files:
- cargo-deny-triage.json - normalized JSON array of diagnostics (produced by tools/triage_cargo_deny.py)
- cargo-deny-classification.json - per-advisory classification (fix-locally / upstream-required / accept-exception)
- deny-suggestions.toml - suggested entries to add to deny.toml for low-risk exceptions (draft)
- cargo-update-cmds.txt - suggested `cargo update` or upstream-issue actions

Next steps:
1. Review `cargo-deny-classification.json` and confirm classifications.
2. For `fix-locally` items, run the suggested `cargo update -p <pkg>` and run tests.
3. For `upstream-required` items, open issues/PRs against Solana/Anchor or wait for upstream to release patched versions.
4. For accepted exceptions, copy entries from `deny-suggestions.toml` into `deny.toml` with justification and expiry.
