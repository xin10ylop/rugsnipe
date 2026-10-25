# Who he follows (docs/16), 2026-10-25

Scripts behind docs/16, run from the session scratchpad (`SP`): they read the create universe (`univ/`), the daily
datasets (`pm/dataset_*.json`), the wallet's month (`month/`) and the tapes they pull (`full/`, `fp/`, `dec/bwr/`).
Order: `fptape.py`, `rest_bw.py`, `rest_rule.py` (tapes and create slots), `ftrace.py` (funders); `ev.py` (the ex-ante
follow test), `lag.py`, `ident.py`, `need.py` + `needtape.py` (full tapes of the followed coins), `final.py` (the four modes).
`d0`-`d9`, `fa`, `link`, `who`, `ops`: the signals that do not explain the picks (docs/16 section 2) and the operators.
Last night (docs/16 section 6): `night.py` + `night_fast.py` (his attempts from Oct 24 12:00, the coins' create slots and
the night's 12-slot tapes), `night_wbuys.py` (each bundle wallet's real launches: the distinct coins it bought; clustering
its transactions counted funding and sweeps as launches, about 3x too many), `nightlead.py`, `nightev.py`.
