# The optimized copy (listed signers only, sold one slot before his table hold) on the coins the newest bot does not
# fire (the exact replay, Oct 23 -> now), by his size and tip; our measured landing distances, 0.5 SOL, bound +10%.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/exitopt.py").read().split("for sl, sel in")[0]
exec(src)
for prem in (0.10, 0.05, 0.20):
    PREM = prem
    print(f"\n== bound +{prem:.0%} over his price")
    for lab, f in (("all his buys", lambda r: True), ("his buy >= 0.4 SOL", lambda r: r["sol"] >= 0.4), ("his buy >= 0.7 SOL", lambda r: r["sol"] >= 0.7),
                   ("his tip >= 4M", lambda r: r["tip"] >= 4e6), ("his tip >= 8M", lambda r: r["tip"] >= 8e6)):
        sel = [r for r in rows if r["listed"] and f(r) and r["bt"] >= T0 and not r["fired"]]
        run(f"{lab}, listed, not fired by the bot (Oct 23+)", sel, lambda r: r["H"] - 1)
    sel = [r for r in rows if r["listed"] and r["sol"] >= 0.7]
    run("his buy >= 0.7 SOL, listed, all 5 days (any)", sel, lambda r: r["H"] - 1)
