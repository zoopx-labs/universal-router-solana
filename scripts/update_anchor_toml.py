#!/usr/bin/env python3
import sys
from pathlib import Path
import re

if len(sys.argv) < 2:
    print("Usage: update_anchor_toml.py PROGRAM_ID", file=sys.stderr)
    sys.exit(2)

prog = sys.argv[1]
p = Path('Anchor.toml')
if not p.exists():
    print('Anchor.toml not found', file=sys.stderr)
    sys.exit(1)

txt = p.read_text()
if re.search(r"\[programs\.devnet\]", txt):
    # replace existing universal_router_sol line if present
    if re.search(r"universal_router_sol\s*=\s*\"[^"]+\"", txt):
        txt = re.sub(r"(universal_router_sol\s*=\s*)\"[^"]+\"", r"\1\"{}\"".format(prog), txt)
    else:
        # add the line under the existing section
        txt = re.sub(r"(\[programs\.devnet\].*?)(\n\[|$)", lambda m: m.group(1) + f"\nuniversal_router_sol = \"{prog}\"\n" + ("\n[" if m.group(2).startswith("[") else ""), txt, flags=re.S)
else:
    txt = txt + f"\n[programs.devnet]\nuniversal_router_sol = \"{prog}\"\n"

p.write_text(txt)
print('Anchor.toml updated')
