# Triage & Operator Runbook

This runbook describes how to re-run supply-chain scanners, interpret the generated artifacts, accept or rotate `cargo-deny` exceptions, and open upstream issues for transitive advisories.

## Where artifacts live

- Raw cargo-deny NDJSON (CI output): `cargo-deny-advisories.json` (root of workspace during CI run)
- Normalized triage JSON: `triage/cargo-deny-triage.json`
- Classification and suggested actions: `triage/cargo-deny-classification.json`
- Suggested deny exceptions (draft): `triage/deny-suggestions.toml` and `deny-exceptions/deny-suggestions.toml`
- Suggested cargo update commands: `triage/cargo-update-cmds.txt`

CI uploads `triage/cargo-deny-triage.json` and a human-readable `deny.txt` as build artifacts in `.github/workflows/supply-chain.yml`.

## Re-running scanners locally

1. Create and activate the Python virtualenv used by tooling (optional for triage scripts):

   python -m venv .venv
   source .venv/bin/activate
   pip install -r tools/requirements.txt  # if present

2. Run cargo-deny and capture NDJSON:

   cargo deny check --report ndjson > cargo-deny-advisories.json || true

3. Normalize and classify with our tools:

   .venv/bin/python tools/triage_cargo_deny.py
   .venv/bin/python tools/classify_triage.py

4. Inspect `triage/cargo-deny-classification.json` and `triage/deny-suggestions.toml`.

## Interpreting classifications

- `upstream-required` — the advisory requires an upstream upgrade or patch. Drafts for upstream issues exist under `triage/upstream_issues/`.
- `fix-locally` — we can attempt a local `cargo update -p <pkg>` or patch in-tree and run the test suite.
- `accept-exception` — short-lived, low-risk dev-time crate; if accepted, add a `[[exceptions]]` entry to `deny.toml` with an `owner` and `expiry` and push via PR.

When in doubt, escalate to `security@zoopx-labs` (owner listed in current exceptions) and include the triage JSON artifact.

## Adding or rotating deny exceptions

1. Prepare a PR that updates `deny.toml` with a new `[[exceptions]]` entry. Required fields:

   - `name` — crate name
   - `version` — semver (e.g., "=0.12.1")
   - `owner` — team or email (e.g., `security@zoopx-labs`)
   - `expiry` — ISO date (e.g., `2025-12-31`)

2. In the PR description, include:
   - Link to `triage/cargo-deny-triage.json` produced by CI
   - Rationale (why exception is safe or temporary)
   - Proposed follow-up action (e.g., open upstream issue, schedule upgrade)

3. Assign `security@zoopx-labs` and at least one code owner for review. Merge only after one security approver and one reviewer approve.

4. When an upstream fix is available, create a follow-up PR that removes the exception and upgrades the dependency. Reference the original exception PR.

## Opening upstream issues

Drafts for upstream issues were created under `triage/upstream_issues/`. To open an issue:

1. Copy the draft into the upstream project's issue tracker.
2. Attach the relevant section(s) from `triage/cargo-deny-triage.json` and `triage/cargo-deny-classification.json` to illustrate impact.
3. Provide a small reproduction or test-case if possible, and suggest the minimum upgrade or mitigation (see `triage/cargo-update-cmds.txt`).

## Owner & cadence

- Short-term exception owner: `security@zoopx-labs` (review quarterly or on-event)
- Triage lead: engineering on-call for this repo (rotate weekly)
- Review cadence: exceptions should be revisited within 90 days of approval or sooner if new advisories appear.

## Quick commands

- Normalize raw NDJSON into JSON/CSV:
  .venv/bin/python tools/triage_cargo_deny.py
- Classify and generate suggestions:
  .venv/bin/python tools/classify_triage.py
- Propose local fixes:
  # Example: update ouroboros
  cargo update -p ouroboros

## Notes

- Keep exceptions minimal, time-limited, and documented with owner/expiry.
- Prefer patching or upgrading dependencies over exceptions.
