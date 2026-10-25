# Who he follows (docs/16), 2026-10-25

Scripts behind docs/16, run from the session scratchpad (`SP`): they read the create universe (`univ/`), the daily
datasets (`pm/dataset_*.json`), the wallet's month (`month/`) and the tapes they pull (`full/`, `fp/`, `dec/bwr/`).
Order: `fptape.py`, `rest_bw.py`, `rest_rule.py` (tapes and create slots), `ftrace.py` (funders); `ev.py` (the ex-ante
follow test), `lag.py`, `ident.py`, `need.py` + `needtape.py` (full tapes of the followed coins), `final.py` (the four modes).
`d0`-`d9`, `fa`, `link`, `who`, `ops`: the signals that do not explain the picks (docs/16 section 2) and the operators.
Last night (docs/16 section 6): `night.py` + `night_fast.py` (his attempts from Oct 24 12:00, the coins' create slots and
the night's 12-slot tapes), `night_wbuys.py` (each bundle wallet's real launches: the distinct coins it bought; clustering
its transactions counted funding and sweeps as launches, about 3x too many), `nightlead.py`, `nightev.py`.
The fee-routing key and what is left (docs/16 section 8): `evkey.py` (wallet alone vs (wallet, fee routing), run from
`SP/dec`); `cell_rows.py`, `cell_val.py`, `cell_dyn.py` (last night per operator and fee routing: the take sequences, the
coins' value behind the bundle, his take after his own win / loss; run from `SP/dec/cell`, they write there);
`cell_2324.py` (Oct 23-24 cells with the tweet features), `authkey.py` (the tweet author as a key), `gaps.py` (his attempts
and profit by what the keyed follow knew), `skipval.py` (followed coins he skipped vs took), `onb.py` (his first attempt in
a cell), `nobundle.py` (coins with no bundle: dev-wallet reuse), `memory.py` (his attempts before Oct 23 on the wallets of
the picks the follow missed, against a control), `leader.py` (create-slot wallets on his unexplained picks vs his skips),
`exit.py` (his sell slot learned per cell vs the live exit). `seed.py` writes `data/follow_seed.json`, the bot's warm start.
Section 9: `today.py` (today live through the seed), `prebuy.py` + `prebuy_cmp.py` + `structure.py` + `bfund.py` (the
full pre-buy state of last night's launches and what separates his takes inside an operator), `evval.py` (following and
sizing by the cell's recent coin value), `hist_depth.py` + `hist_older.py` + `memory2.py` (his whole history since Aug
against the bundle wallets' whole histories). All run from the scratchpad; `prebuy_cmp.py` and `structure.py` from `SP/dec/cell`.
