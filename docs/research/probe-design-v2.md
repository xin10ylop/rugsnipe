# Probe design v2: a probe that can answer its question

Date 2026-10-16. Replaces the two-stage probe of `docs/04-strategy.md` §7 item 2 (v1: 0.02 SOL, 300 attempts or
3 days, then 0.25 SOL to 200 landed trades). The **kill rule is unchanged** (docs/04 §7, docs/08 §10); this
document only changes how attempts are spread, how many there are, which mode the filter runs in, how landing is
calibrated and how stage B is read. Inputs: the 14-hour shadow run (3,091 attempts, EV per attempt **+1.32%**
[+0.87, +1.77]; buyer-behind +4.41%, none −1.50%; filter tertiles −1.64% / +1.06% / +7.22%, skip-mode **≈ +4.2%**;
8% unresolved), the modelled failed-attempt cost **0.00073 SOL** (docs/04 §4) and `pnl_model.py`.

## 1. What was wrong with v1

* **Clustered attempts.** 50 attempts a day fire in whatever hours the launches come; a 3-day stage A is a handful of
  busy hours, and docs/04 §3 shows the policy ranges from +0.1% to +6.1% between same-day windows. The landing rate
  and premium of three days are one regime's numbers.
* **Too few attempts for the break-even.** At a 10% landing rate, 300 attempts give a 90% CI of ±2.8 points on the
  landing rate, the same size as the gap between the break-even (9.9% at 0.25 SOL) and a no-go.
* **Filter in the wrong mode.** Stage A ran with an empty outcome store: every signer first-seen, so skip mode
  would have scored nothing for days, and the measured landing rate would be on launches the live policy skips.
* **Stage B read in isolation.** 200 landed trades at a per-trade SD of 19.8% leave a ±2.3-point CI; nothing tied
  the real trades to the shadow estimate on the same launches.

## 2. Stage A: 0.02 SOL, thinned, filtered, 7-10 days

> **Readiness audit M4 (2026-10-20, `docs/research/readiness-2026-10-20.md` §2). Items 1-3 and 5-6 follow
> from the build and the audit; item 4 was decided on 2026-10-20 (option ii, the 6-per-hour row; the owner can
> change it before the first real start by editing the three config lines together).** Where this block and the table
> below differ, the block describes what the build and `config/probe-a.example.toml` actually do.
> 1. **Probe A runs unfiltered on every class-eligible create.** The build cannot apply the skip filter or a
>    multi-bundle gate before it sends: both act only on paper intents (crates/rugsnipe/src/hot.rs:1064-1101), so
>    `filter_mode = "skip"`, the multi-bundle universe and the outcome-store pre-warm have no effect on real buys.
>    The probe config is `config/probe-a.example.toml` (`[entry] filter = "any"`).
> 2. **`[feeds] grpc.bundle_run` stays on**, so every attempt gets a `feed_bundle` row with `bundle_wallets`.
> 3. **The stage-B entry is read on the in-class subset only** (`feed_bundle.bundle_wallets >= 1`), never on all
>    attempts: unbundled launches face fewer rivals and fill easily, so the all-attempt landing rate is inflated.
> 4. **How stage A reaches 200 in-class attempts (decided, see the end of this item).** Only
>    10-20% of attempts are in class, so 200 in-class attempts take **1,000-2,000 attempts whatever the rate**.
>    An attempt is expected to cost 0.0013-0.0022 SOL (20-50% landing; a landed 0.02 SOL trade −17.9% = 0.00358 SOL,
>    §4; a failed one 0.00073 SOL), so the subset costs about **1.3-4.3 SOL in total** under any option. The rate only
>    trades calendar days against the daily loss:
>
>    | `risk.max_buys_per_hour` | attempts a day | expected loss a day (20-50% landing) | days to 200 in-class (20% / 10% in class) | `a.max_daily_loss_lamports` it needs |
>    |---|---|---|---|---|
>    | 2 | 48 | 0.06-0.10 SOL | 21 / 42 | 0.2 SOL |
>    | 3 | 72 | 0.09-0.16 SOL | 14 / 28 | 0.25-0.3 SOL |
>    | 4 | 96 | 0.12-0.21 SOL | 11 / 21 | 0.35-0.4 SOL |
>    | **6 (chosen)** | 144 | 0.19-0.31 SOL | 7 / 14 | 0.6 SOL (shipped) |
>
>    The cap needs clear headroom over the expected loss at a high landing rate: one ordinary day at the cap latches
>    `daily_loss` and the gate reads `KILL` (a false kill caused by config). The 0.2 SOL cap therefore carries 2
>    attempts an hour (3 only at a low landing rate). `a.max_probes_per_day` must be at least 24 × the hourly cap,
>    or the evening hours are never sampled (readiness M3b).
>    - **The shipped file** (`config/probe-a.example.toml`): 6 per UTC hour, 150 a day, 0.6 SOL daily loss, 3 SOL
>      funded plus top-ups, hard stop at 14 days (`--hard-stop-days 14`), ArmSet signed for 15 days
>      (`--valid-hours 360`, `armset_max_age_s = 1_296_000`). A subset still below 200 at the hard stop reads
>      `INCONCLUSIVE: ... escalate to the owner`, not a stop.
>    - **(i) more calendar days** at the shipped caps (plan 21-42 days): sign the ArmSet for the planned length
>      (`--valid-hours` = 24 × (days + 1), `armset_max_age_s` = 3,600 × that), pass `--hard-stop-days DAYS` to the
>      gate (docs/08 §9 step 7), and top the wallet up as in docs/08 §9 step 7b (about 0.06-0.10 SOL a day).
>    - **(ii) a higher rate:** change `risk.max_buys_per_hour`, `[probe] a.max_probes_per_day` and
>      `a.max_daily_loss_lamports` together, from one row of the table; fund for the row's daily loss times its days
>      (or top up per step 7b); pass `--hard-stop-days` if the row needs more than 10 days.
>    - **(iii)** delay stage A for a pre-send gate (days of Rust).
>
>    **Decision (2026-10-20, made by the operator's delegate under the standing "make it your way" mandate):
>    option (ii), the 6-per-hour row.** Reason: the total cost of reaching 200 in-class attempts is the same at
>    any rate (1.3-4.3 SOL), so the faster rate only saves box-days (7-14 instead of 21-42 at about $40 a day) and
>    gets to the stage-B decision sooner; the 0.6 SOL daily cap keeps a bad day small. The owner can override it
>    before the first real start by editing the three config lines together.
> 5. **Counted as landed:** only creation-slot fills at a premium of +2% or less; slot+1..+3 fills are reported
>    apart. Compute or instruction errors (`fills.fail_reason`) are not lost races. The B bar is the larger of the
>    pooled LEAD break-even (about 9.9%) and the unfiltered shadow break-even at 0.25 SOL (about 36%, §4).
>    Completion: ≥ 350 attempts and ≥ 7 days, and `shadow_gate.py --probe` waits for 200 in-class attempts before
>    the B read. At the hard stop a subset still below 200 that meets every other B condition (or holds no attempt)
>    reads `INCONCLUSIVE ... escalate to the owner`: too little data is not a negative result, so it is never a
>    `stop`.
> 6. **The pre-send gate** (a create-only filter or a multi-bundle proxy visible at fire time) is a before-stage-B
>    item (§9); without it stage B faces the unfiltered 36% bar.

| Setting | v1 | **v2** |
|---|---|---|
| stake | 0.02 SOL | 0.02 SOL |
| attempts | ≤ 50/day, first come | **≤ 6 per UTC hour, ≤ 150/day**, first come within the hour (decision of 2026-10-20 above; the quarter-hour spacing is a before-stage-B item) |
| duration | 300 attempts or 3 days | **7 to 14 days**, ends at ≥ 350 attempts, ≥ 7 days and ≥ 200 in-class attempts; hard stop at 14 days (`--hard-stop-days 14`) |
| filter | log | **log (unfiltered)** in stage A: the skip filter acts only on paper intents in this build (hot.rs:1064-1101); the create-only model becomes the live filter before stage B, once the hot extractor emits `intx_n_buys` |
| outcome store | empty | **pre-warmed from the shadow state.db** (`prewarm_outcomes.py`) |
| kill | docs/04 §7 | **unchanged** |

The hourly thinning spreads ≥ 350 attempts over ≥ 150 distinct UTC hours (every hour of the day about 7 times), so
the hour-cluster bootstrap has enough clusters and the landing rate is a day-average, not a busy-hour number.
`shadow_gate.py --probe --config /etc/rugsnipe/rugsnipe.toml` flags any UTC hour above the config's `risk.max_buys_per_hour` (a config or scheduler bug, not a kill).

**What A measures:** landing rate per leader class and per sender path, the premium (mean and median), the realised
failed-attempt cost, which bid variant lands. Its EV is negative by construction (a 0.02 SOL trade carries the
0.004 SOL round trip, −15 to −18 points of stake) and that is not a kill reason.

**Pre-warm.** The live filter's operator features come from `outcomes` (our own LEAD outcome per launch, per signer
and bundle wallet). Copy the shadow run's rows into the probe file before the first start:

```bash
# on the box, with rugsnipe-shadow stopped (or on a copy of its state.db)
cd /opt/rugsnipe/src
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/prewarm_outcomes.py --src /var/lib/rugsnipe/shadow/state.db \
    --dst /var/lib/rugsnipe/state.db --max-age-days 14 --dry-run     # counts only
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/prewarm_outcomes.py --src /var/lib/rugsnipe/shadow/state.db \
    --dst /var/lib/rugsnipe/state.db --max-age-days 14
```

Rows are copied unchanged with `intent_id` negated (no collision with the probe's own intents, idempotent on
re-run); a missing `--dst` is created with the schema's DDL and version; `kv.outcomes_prewarmed_from` records the
source. Every copied row predates the probe, so the history a probe decision sees is still point in time.

## 3. Index probes: landing calibration per sender path

Cheap "index probes" of **0.001-0.005 SOL** on the same thinned schedule, interleaved with stage A (at most one per
UTC hour per sender path, about 20 a day per path), measure only *whether and where* a buy lands: our transaction
index in slot 0 against the bundle end and the first third-party buyer, per sender path (direct TPU, each relay) and
leader class. Same `max_sol_cost` cap and the same bid as a real attempt, so the landing behaviour and the failed
cost are the real ones; only the stake is smaller. They are not counted in A's EV or landing-rate kill (the kill
reads the 0.02 SOL attempts only; tag them `intents.mode = "index"` or a separate `[probe] index` lane in the hot
config, a Rust-side item). After 7 days each path has about 140 index probes: enough to rank paths (a 10-point
landing difference is visible at 90%) and to choose the variant for stage B. The bid is the same at any stake, so a
failed index probe costs the same as a failed 0.02 SOL attempt; budget them (20/day/path × 0.00073 SOL ≈ 0.015 SOL a
day per path).

## 4. Break-even landing rates (pnl_model)

`pnl_model.break_even_at_stake(ev, 0.5, stake, c_fail=0.00073)`: the shadow EV is moved from 0.5 SOL to the stake
by the fixed 0.004 SOL round trip only (premium and exit treated as size-free), every failed attempt pays 0.00073
SOL (f_pay = 1). The shadow +1.32% is an EV **per attempt** (failures included), used here as if it were per landed
trade, which is conservative; `shadow_gate.py` now prints the per-landed-trade EV (rule (b) proxy), which is the
right input (`--shadow-ev`).

| Stake (SOL) | EV per landed trade from +1.32% | break-even landing | from +4.2% (skip mode) | break-even landing |
|---|---|---|---|---|
| 0.005 (index) | −77.9% | none | −75.0% | none |
| 0.02 (stage A) | −17.9% | none | −15.0% | none |
| 0.10 | −1.9% | none | +1.0% | 42.2% |
| **0.25 (stage B)** | +0.5% | **36.0%** | +3.4% | **7.9%** |
| 0.50 | +1.3% | 10.0% | +4.2% | 3.4% |

For comparison the docs/04 §7 entry rule (LEAD 0.25 SOL, pooled OOS +2.7%, at 0.00073 SOL): 9.9%. Reading: stage B
at 0.25 SOL is worth entering only in skip mode, and only if A's landing rate clears about 8-10%; on the
unfiltered shadow EV it needs 36%, which nothing in gap-2 suggests we reach. `shadow_gate.py --probe --shadow-ev
<per-landed EV>` prints these numbers from the measured failed cost.

## 5. Stage B: 0.25 SOL, paired with shadow, sequential stopping

**Entry** (unchanged in substance, stricter in input): A's landing rate above the docs/04 §7 break-even
(`pnl_model.break_even_landing`, 0.25 SOL LEAD at the measured mean failed cost and the **mean** measured premium,
f_pay = 1) **and** above the skip-mode shadow break-even of §4, and A's median premium ≤ +2%.

**Paired design.** The shadow lane keeps running on the same feed (paper intents next to the real ones,
`intents.mode = 'paper'` vs `'probe'`, which the gate already separates). Every real attempt has a shadow twin on
the same launch, so the comparison is per launch: d = real return − shadow return of the same launch. This removes
the launch-to-launch variance (SD 19.8% per trade) that dominates an unpaired read and isolates what shadow cannot
know: our real rank, premium and exit slip. Report mean d with a launch-level and an hour-clustered CI; the shadow
estimate is trusted for live sizing only if mean d's 90% CI includes 0 or is above −1 point.

**Sequential stopping (on the per-attempt EV, failures included).** Look after every 50 landed trades
(looks at 50, 100, 150, 200, 250, 300; max 300 landed or 14 days). At each look compute the 90% hour-cluster CI
widened by an alpha-spending factor (O'Brien-Fleming-like: z = 1.645 × sqrt(K / k) for look k of K = 6, i.e. 4.03,
2.85, 2.33, 2.01, 1.80, 1.645):

* **stop for futility / kill** when the upper bound is below 0 (and, independently, every docs/04 §7 kill condition
  at its own threshold: EV upper bound < 0 after 100 landed or 400 attempts at the plain 90% level, median premium
  > +3%, landing below break-even over 200 attempts, the daily loss cap). The spending rule never delays a kill
  that the docs/04 rule would fire;
* **stop for success** when the lower bound is above 0 at a look ≥ 150 landed trades and the paired mean d is
  within the band above; then read the live gate (docs/06 §9.5);
* otherwise continue to the next look; at 300 landed trades without a success stop, the answer is "not shown", no
  live.

Stage B caps stay as in v1 (≤ 300 attempts/day, 2 open positions, 1 SOL daily loss cap), plus the 4-per-UTC-hour
thinning for the first 100 attempts so the first looks are not one busy evening.

## 6. Kill rule (unchanged)

docs/04 §7 item 2 and docs/08 §10 as written: (B) EV per attempt 90% upper bound < 0 after 100 landed or 400
attempts; median realised premium > +3%; landing rate below the measured-cost break-even over 200 attempts; the
stage's daily loss cap (latched by the risk gate); plus every kill-switch trigger of docs/08 §10. The only change in
`shadow_gate.py` is that the break-even is priced at the mean measured premium instead of the median (a stricter or
equal input whenever the premium distribution is right-skewed); the premium thresholds remain on the median.

## 7. Alpenglow schedule

Alpenglow activates at **epoch 1064, about Oct 21 2026 15:00 UTC**. Slot timing, leader scheduling and landing
change, so pre- and post-cutover numbers are not pooled.

| Date (UTC) | Action |
|---|---|
| Oct 16-20 | shadow keeps running (pre-cutover baseline, ≥ 4 more days); weekly gate read on Oct 20 |
| **Oct 21 ~12:00** | **stop shadow** before the cutover; archive `state.db` and the summary (pre-Alpenglow set) |
| Oct 21 15:00 | epoch 1064 activates; do not trade or shadow through the transition |
| Oct 22 00:00 → Oct 23 00:00 | **post-cutover shadow day** on a fresh `state.db`: re-measure ms per slot (the gate now prints it from the wall clock), premium, unresolved share, tertile split |
| **Oct 23 or later** | **start probe A** if the post-cutover shadow day is not worse than the pre-cutover run (EV per attempt CI overlaps, unresolved ≤ 10%); pre-warm from the post-cutover shadow file plus the pre-cutover one (outcomes are wallet history, not timing-dependent) |
| A + 7-10 days (≈ Oct 30 - Nov 2) | stage A read: enter B, stop, or `INCONCLUSIVE` (in-class subset short of 200: the owner extends A, §2 M4 item 4) |

## 8. Commands (for docs/08)

```bash
cd /opt/rugsnipe/src
# shadow gate (wall-clock hours; summary must match the file; signer clusters next to hour clusters)
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py --state /var/lib/rugsnipe/shadow/state.db \
    --summary /var/lib/rugsnipe/shadow/summary.json --cluster signer
# probe stage A read (in-class split, stage-B bar, late fills, by sender; the hour cap is the config's
# risk.max_buys_per_hour; add --hard-stop-days DAYS when the owner extends stage A, section 2 item 4)
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py --probe --state /var/lib/rugsnipe/state.db \
    --config /etc/rugsnipe/rugsnipe.toml --cluster signer
# pre-warm the probe's outcome store (section 2; no effect on real buys in this build, see the M4 block)
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/prewarm_outcomes.py --src /var/lib/rugsnipe/shadow/state.db \
    --dst /var/lib/rugsnipe/state.db --max-age-days 14
```

## 9. Rust-side items this design needs (not done here)

> **Readiness audit M4 (2026-10-20):** none of these is a prerequisite for stage A any more: A runs unfiltered on
> every class-eligible create (§2 M4 block), so the skip filter, the thinning scheduler and the index lane are not
> needed to start it. The **pre-send entry gate** (a create-only filter with `intx_n_buys`, or a multi-bundle gate
> or proxy visible at fire time) is a **before-stage-B** item: without it stage B is read against the unfiltered
> break-even of about 36% at 0.25 SOL instead of about 7.9%.

* the hourly thinning (≤ 4 per UTC hour, first qualifying launch per quarter hour) in the probe scheduler;
* an index-probe lane (`intents.mode = "index"`, 0.001-0.005 SOL, per sender path);
* `intx_n_buys` in the create-tx feature extractor (for `entry_filter_v1_create_only.json`);
* `shadow.err TEXT` (the E1 lane's failure string) so the gate can split unresolved by RPC phase; today only
  `create_index IS NULL` (slot 0 never read) vs set (replay tape cut) is available;
* `started_unix` and `state_path` in the end-of-run summary so the gate can tie a summary to its file directly.
