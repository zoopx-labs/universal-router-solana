#!/usr/bin/env python3
import json
import os
from pathlib import Path
import csv

ARTIFACT = Path("/home/zoopx/zoopx/solana/cargo-deny-advisories.json")
OUTDIR = Path("/home/zoopx/zoopx/solana/triage")
OUTDIR.mkdir(exist_ok=True)
OUT_JSON = OUTDIR / "cargo-deny-triage.json"
OUT_CSV = OUTDIR / "cargo-deny-triage.csv"


def read_ndjson(path: Path):
    objs = []
    with path.open("r", encoding="utf-8") as f:
        for i, line in enumerate(f, 1):
            line=line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError as e:
                # If file contains backticks or fenced code blocks (from earlier capture), try to strip leading/trailing ticks
                cleaned = line.strip('`')
                try:
                    obj = json.loads(cleaned)
                except Exception:
                    print(f"Failed to parse line {i}: {e}\nLINE: {line[:200]}")
                    raise
            objs.append(obj)
    return objs


def extract_summary(diags):
    rows = []
    for d in diags:
        fields = d.get("fields", {})
        adv = fields.get("advisory", {})
        code = fields.get("code") or d.get("type") or ""
        msg = d.get("message") or adv.get("title") or ""
        pkg = adv.get("package") or adv.get("package")
        advisory_id = adv.get("id") or adv.get("aliases", [None])[0]
        graphs = fields.get("graphs") or []
        # attempt to find the top-level Krate
        top_pkg = None
        if graphs:
            try:
                top_pkg = graphs[0][0]["Krate"]
            except Exception:
                # Try other shapes
                pass
        parents_text = []
        # gather parent chain names
        def walk_graph(g):
            names = []
            def rec(node):
                if isinstance(node, dict) and "Krate" in node:
                    k = node["Krate"]
                    names.append(f"{k.get('name')}@{k.get('version')}")
                    for p in node.get("parents", []) or []:
                        rec(p.get("Krate") if isinstance(p, dict) and "Krate" in p else p)
            try:
                rec(g)
            except Exception:
                pass
            return names
        try:
            for g in graphs:
                parents_text.extend(walk_graph(g))
        except Exception:
            parents_text = []

        rows.append({
            "advisory_id": advisory_id,
            "package": pkg if pkg else (top_pkg and f"{top_pkg.get('name')}@{top_pkg.get('version')}"),
            "code": code,
            "message": msg,
            "severity": fields.get("severity"),
            "notes": "; ".join(fields.get("notes", [])[:3]) if fields.get("notes") else None,
            "parents": " | ".join(parents_text[:6])
        })
    return rows


def main():
    if not ARTIFACT.exists():
        print(f"Artifact not found: {ARTIFACT}")
        return 2
    print(f"Reading {ARTIFACT}")
    diags = read_ndjson(ARTIFACT)
    print(f"Parsed {len(diags)} diagnostic objects")
    rows = extract_summary(diags)
    # write JSON array
    with OUT_JSON.open("w", encoding="utf-8") as f:
        json.dump(rows, f, indent=2)
    # write CSV
    with OUT_CSV.open("w", encoding="utf-8", newline='') as f:
        writer = csv.DictWriter(f, fieldnames=["advisory_id","package","code","message","severity","notes","parents"])
        writer.writeheader()
        for r in rows:
            writer.writerow(r)
    print(f"Wrote {OUT_JSON} and {OUT_CSV}")
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
