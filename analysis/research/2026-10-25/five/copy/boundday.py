# bound.py's fires (Oct 23 -> now) by UTC day, at a flat exit (argv[1], default 9), for max_premium 0 / 5% / 10%.
import json, collections, datetime, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
EX = sys.argv[1] if len(sys.argv) > 1 else "9"
sys.argv = ["x", EX]
src = open(SP + "/dec/behind/bound.py").read()
src = src.replace("if v is not None: out[bound][bucket].append(v); out[bound][\"all\"].append(v)",
                  "if v is not None: out[bound][bucket].append(v); out[bound][\"all\"].append(v); out[bound][datetime.datetime.fromtimestamp(cr[m]['bt'], datetime.timezone.utc).strftime('%m-%d')].append(v)")
exec(src)
for b in (0.0, 0.05, 0.10):
    print(f"max_premium {b:.0%}: " + "  ".join(f"{d} {sum(out[b][d]):+.2f} ({sum(1 for x in out[b][d] if x != 0.0)} fills)" for d in sorted(k for k in out[b] if k[:2] == "10")))
