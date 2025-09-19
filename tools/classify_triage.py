#!/usr/bin/env python3
import json
from pathlib import Path

IN_PATH = Path("/home/zoopx/zoopx/solana/triage/cargo-deny-triage.json")
OUT_DIR = Path("/home/zoopx/zoopx/solana/triage")
OUT_DIR.mkdir(exist_ok=True)
OUT_JSON = OUT_DIR / "cargo-deny-classification.json"
OUT_DENY = OUT_DIR / "deny-suggestions.toml"
OUT_UPDATES = OUT_DIR / "cargo-update-cmds.txt"

HIGH_PRIORITY_CODES = {"vulnerability", "unsound"}

# heuristics for upstream
UPSTREAM_HINTS = ["solana-", "anchor-", "spl-", "solana", "anchor", "ed25519-dalek", "curve25519-dalek", "borsh", "ring"]
DEV_HINTS = ["ansi_term", "atty", "derivative", "paste", "proc-macro-error", "proc-macro2", "syn", "env_logger", "clap"]


def classify_row(r):
    pkg_field = r.get("package") or ""
    pkgname = pkg_field.split("@")[0] if pkg_field else None
    code = r.get("code") or ""
    severity = r.get("severity")
    parents = (r.get("parents") or "").lower()

    # default
    classification = "upstream-required"
    rationale = []

    if code in HIGH_PRIORITY_CODES:
        # high severity -> need fix; but decide direct vs upstream
        # if parents mention solana/anchor => upstream
        if any(h in parents for h in UPSTREAM_HINTS):
            classification = "upstream-required"
            rationale.append("transitively introduced via Solana/Anchor stack")
        elif pkgname and pkgname in DEV_HINTS:
            classification = "accept-exception"
            rationale.append("dev-time crate; likely safe to accept with exception or replace")
        else:
            # treat as fix-locally if appears to be direct dependency
            classification = "fix-locally"
            rationale.append("appears to be directly addressable in workspace")
    else:
        # non-high severity (e.g., unmaintained informational) -> likely accept or replace
        if pkgname and any(d in pkgname for d in DEV_HINTS):
            classification = "accept-exception"
            rationale.append("dev-time/cli/proc-macro; suggest exception or replacement when time permits")
        elif any(h in parents for h in UPSTREAM_HINTS):
            classification = "upstream-required"
            rationale.append("transitively introduced via Solana/Anchor stack")
        else:
            classification = "accept-exception"
            rationale.append("low-impact or informational; accept with documented justification")

    return {
        "advisory_id": r.get("advisory_id"),
        "package": pkgname,
        "code": code,
        "message": r.get("message"),
        "severity": severity,
        "parents": r.get("parents"),
        "classification": classification,
        "rationale": "; ".join(rationale)
    }


def main():
    if not IN_PATH.exists():
        print(f"Input not found: {IN_PATH}")
        return 2
    rows = json.loads(IN_PATH.read_text())
    classified = []
    deny_entries = []
    updates = []
    for r in rows:
        cls = classify_row(r)
        classified.append(cls)
        pkg = cls.get("package")
        aid = cls.get("advisory_id")
        if cls["classification"] == "accept-exception":
            # make a deny.toml exception entry template
            if pkg:
                deny_entries.append(f"[[exceptions]]\npackage = \"{pkg}\"\nreason = \"Accept temporary exception for {aid}: {cls['rationale']}\"\nexpires = \"{2025}-12-31\"\n")
            else:
                deny_entries.append(f"# Unknown package for advisory {aid}; manual review needed\n")
        elif cls["classification"] == "fix-locally":
            if pkg:
                updates.append(f"# Try: cargo update -p {pkg}\n")
        else:  # upstream-required
            if pkg:
                updates.append(f"# Upstream required: open issue/PR to upgrade {pkg} in solana/anchor stack for advisory {aid}\n")

    OUT_JSON.write_text(json.dumps(classified, indent=2))
    OUT_DENY.write_text("\n".join(deny_entries))
    OUT_UPDATES.write_text("\n".join(updates))

    print(f"Wrote classification to {OUT_JSON}, deny suggestions to {OUT_DENY}, updates list to {OUT_UPDATES}")
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
