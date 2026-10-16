# H1: the LEAD entry filter, end to end

**Question.** The angles critic (`strategy-angles-critic.md` §6) adopted "skip the bottom tertile of the frozen
P(take-profit) score" as the LEAD's entry filter for shadow mode and the probe, on two conditions: re-freeze the
model without the inputs a live bot cannot have at slot 0 (metadata JSON, `cu_used`), and keep a live per-signer
store of our own outcomes. This page records that re-freeze, its export, the Rust implementation in the hot path and
a first paper session with the filter in log mode.

*Done 2026-10-14. Code: `analysis/rugsnipe_analysis/strategy/entry_filter.py`, `analysis/scripts/strategy/entry_filter_freeze.py`,
`analysis/rugsnipe_analysis/selftest_entry_filter.py`; `crates/rugsnipe/src/filter.rs` (model, features, outcome store),
`crates/rugsnipe/src/{e1,cold,hot}.rs`, `crates/rugsnipe-state` (schema 3). Artefacts: `data/reports/strategy/entry_filter_v1.json`
(the model the hot path loads), `entry_filter_v1.pkl` (scikit-learn object, sha256 in the JSON), `entry_filter_v1_check.json`
(200 parity rows), `entry_filter_v1_fixtures.json` (real creation slots and create transactions with their Python features).*

## 1. The re-frozen model (`entry_filter_v1`)

* **Same configuration as G1's rule 3**: gradient-boosted trees of depth 3 (scikit-learn `HistGradientBoostingClassifier`,
  learning rate 0.05, 150 trees, min leaf 20, L2 1.0), target = the LEAD's take-profit leg fires first, operator pools
  as extra training rows and as history (`op_all`), sampling weights as in gap-3.
* **Rows: every multi-wallet launch known now**, 3,591: the predictor table (2,965: gap-3, deep, fresh, dense window,
  operator pools) plus the angles critic's holdouts (HA 114 + 146 armed, HB 307, HC 59). The armed HA group, like the
  pools, is a training and history row only. The table's signer funding was re-derived with the fixed `Funding`
  (61 rows had carried funding from after their launch, critic §2.2).
* **60 features, the exact `feature_row` definitions that are computable at the slot-0 decision instant**:
  bundle structure (17), create-transaction fields (14), uri (7), signer funding (10), point-in-time operator
  history (6), time and create rate (6). **Dropped**: metadata JSON (10; fetched ~450 ms after the create), `cu_used`
  (an execution result), the 13 name/symbol text features (Python's Unicode tables are not reproducible bit for bit;
  AUC 0.53-0.59 alone in G1's ablation), `op_g1_prior` (counts creates in the 1.27% gap-1 sample a live bot does not
  have) and `op_busy` (needs 15 minutes of trades per launch; the live store is written 10 s after each launch).
* **Tertile cut points**: weighted 1/3 and 2/3 quantiles of the in-sample predictions on the 2,100 random-sample rows,
  as in G1's freeze: **t1 = 0.1480, t2 = 0.2272** (G1's rule 3: 0.145 / 0.243). In sample the tertiles make −1.6% /
  +1.2% / +12.1% per trade (TP rate 4% / 15% / 50%, AUC 0.84); that is in-sample and says nothing about skill.

**Consistency check before the freeze** (the 60-feature set fitted on the table only, scored once on HA / HB / HC;
those sets were already used by the critic, so this checks the reduced feature set, it is not a clean test):

| Set | n | TP AUC | EV all | skip bottom tertile: kept, EV kept, lift | low / mid / high tertile EV |
|---|---|---|---|---|---|
| HA | 114 | 0.77 | +6.1% | 58%, +9.1%, **+3.1** | +1.9 / +3.7 / +12.9% |
| HB | 307 | 0.69 | +2.8% | 64%, +4.1%, **+1.3** | +0.6 / −0.3 / +8.1% |
| HC | 59 | 0.80 | −0.4% | 59%, −0.3%, **+0.1** | −0.5 / −2.7 / +2.2% |
| pooled | 480 | **0.72** | +3.2% | 62%, **+4.7%**, **+1.5** | +0.8 / +0.2 / +8.6% |
| pooled, funding blanked (a live bot rarely knows it) | 480 | 0.73 | +3.2% | 66%, +4.4%, +1.2 | +0.9 / +0.4 / +8.5% |

The reduced set keeps the frozen rule's lift (critic: +1.4 at 74% kept). Its bottom tertile is not negative here
(+0.8% against the frozen rule's −0.8%); the gain comes from the top third. Blanking the funding block, which a live
bot knows only for signers its funding watcher saw funded, costs 0.3 points and moves 4% of launches out of the
bottom tertile.

**Export.** `entry_filter_v1.json` holds the feature list and groups, the dropped features and why, the defaults of
unknown values, `scaler: null` (trees), the 150 trees as explicit node lists (`feature`, `threshold`, `missing_left`,
`left`, `right` or a leaf `value`; `P = 1 / (1 + exp(-(baseline + sum of leaves)))`), the tertile cut points and every
constant the features need (the five uri hosts, the eight Jito tip accounts, the CEX and swap funders of the label
set, the compute-budget program, the base fee, the regime slot, the 10 s outcome delay). **Exporter test**
(`python -I -m rugsnipe_analysis.selftest_entry_filter`): the pickle matches the JSON's sha256; on 200 seeded rows the
feature vectors recomputed from disk equal the exported ones, and scikit-learn, the pure-Python scorer of the JSON and
the stored scores agree to **0.0** (1e-9 required); NaN routing follows `missing_go_to_left`.

## 2. The filter in the hot path (`crates/rugsnipe/src/filter.rs`)

**Where each feature comes from.**

| Group | Paper (E1 lane) | Live, from the shreds |
|---|---|---|
| bundle (17) | the reconstructed creation slot; the gap-3 bundle with C = {creator, signer, fee payer} exactly as `tapes._raw_bundle` + `bundle_features` (the lane's sniper rule is not applied to the features) | **not hot-visible in full**: the run behind the create (index ≤ create + 5, gaps ≤ 2) streams in with the slot's entries; a shred-level slot-0 assembler has to score once the run is closed |
| create tx (14) | the create's jsonParsed `getTransaction` (tips to the 8 Jito accounts, `meta.fee` − 5,000 × signatures, compute-budget limit and price, instruction count, version, signers) and its `getBlock` index | the transaction itself (priority from the compute-budget price × limit); `tx_index` only approximately (entry position) |
| uri (7) | the CreateEvent's uri | the create instruction: hot-visible |
| funding (10) | the funding watcher's `Funded` entry for the signer (CEX / swap / other funder, fresh, lag), else unknown | same: hot-visible |
| operator (6) | the outcome store (below) | same: hot-visible |
| time (6) | block time, regime, creates of the previous 60 / 600 s (hot-thread ring; the training default until the session has run that long) | same: hot-visible |

So in paper mode the filter runs on the E1 lane's result; live it needs the bundle run from the shreds, which this
build does not assemble yet (the real fill lane is open item M4).

**Flow.** The cold thread assembles the 60 features when the E1 verdict arrives (point in time: the outcome store as
of that moment, without this launch) and sends them right before the verdict (`HotIn::E1Feat`). The hot thread scores
them when the verdict arrives, before the position opens: rslog `Filter` (score, tertile, mode, dropped, the signer's
and best bundle wallet's outcome counts) and a `FilterScore` to the cold thread (state.db `filter` row with the feature
vector). With `entry.filter_mode = "skip"` an attempt (fill, premium, not-in-slot-0) scoring below t1 becomes
`filtered`: no position, no fill row, no cost. `log` (default) changes nothing. `skip` without a model is refused at
startup. Config: `entry.filter_model` (loaded once; a feature list or constants other than this binary's are refused)
and `entry.filter_mode`.

**Outcome store.** Every paper launch we decide on (in the entry class) becomes an `outcomes` row: the signer, the
bundle wallets, the third-party slot-0 buyers, and, for a closed position, the LEAD result (return net of the round
trip per staked SOL, whether the take-profit was the first exit). A row enters the store, and state.db, 10 s after its
launch at the earliest (the training's point-in-time rule); the store keeps per-signer and per-bundle-wallet totals
and is rebuilt from state.db at start. Live limitations: failed attempts have no LEAD result (the training simulated
one for every launch), so `op_lead_*` average over fewer launches than in training; `op_prior` counts every launch the
bot saw, where the training counted launches in a sample; in `skip` mode a skipped operator's history stops growing.

**Speed.** `Model::score` walks fixed node arrays; no allocation (checked under the counting allocator, also inside
the E1 zero-allocation replay). Release build (`cargo run --release -p rugsnipe --example filter_bench`): **mean 1.3 us,
p50 1.2 us, p90 1.4 us, p99 2.0 us** per score (150 trees, 200 rows × 20,000).

**Tests.**

| Test | What |
|---|---|
| `tests/filter_parity.rs` | Rust score = scikit-learn on the 200 exported rows (max diff 5.6e-17) and the same tertiles; bundle features of 60 real HB creation slots and create-transaction / uri features of 24 real create transactions equal the Python pipeline's to 1e-9; no allocation, under budget |
| `tests/filter_pipeline.rs` | skip mode drops exactly the two bottom-tertile attempts (a fill and a premium) and lets a top-tertile and an unscored intent trade; log mode trades all four; state.db `filter` rows, rslog `Filter` records, summary tertile totals, `outcomes` rows. Outcome store through SIGKILL: a child process writes four launches, is killed with SIGKILL, the store rebuilt from state.db equals the expected one and the restart reloads it |
| `src/filter.rs` unit tests | model loading and refusal, point-in-time assembly (a launch's outcome is history only once due), per-bundle-wallet totals, create-rate windows, time and uri features |
| `rugsnipe-state` | schema 3 (`filter`, `outcomes`) round trip |

## 3. Paper session with the filter in log mode

18:22:37-18:37:37 UTC, `config/paper-lead.example.toml` as shipped (E1 lane, LEAD exits, `filter_mode = "log"`),
a fresh state.db (empty outcome store), this container's public-RPC feed (about a second late, so no latency
information). Summary: `data/reports/strategy/entry_filter_v1_session.json`.

* 607 creates, 335 paper intents, 306 E1 verdicts: 232 `class` (not multi-wallet), 18 unresolved, **56 attempts**
  (48 fills, 8 not-in-slot-0), all 56 scored; 250 verdicts without creation-slot features (class / unresolved).
* Every attempt also produced a `filter` row with its feature vector, an rslog `Filter` record and, 10 s after the
  launch, an `outcomes` row (56 rows; 54 signers, 199 bundle wallets).

| Tertile | Attempts (share) | Mean score | Fills / failed | TP exits | Net | **EV per attempt** |
|---|---|---|---|---|---|---|
| low (< 0.148) | 9 (16%) | 0.076 | 8 / 1 | 0 | −0.152 SOL | **−3.4%** |
| mid | 30 (54%) | 0.194 | 25 / 5 | 4 | −0.593 SOL | **−4.0%** |
| high (≥ 0.227) | 17 (30%) | 0.283 | 15 / 2 | 1 | +0.113 SOL | **+1.3%** |
| all | 56 | | 48 / 8 | 5 | −0.632 SOL | −2.3% |

An independent recomputation from state.db (`filter` x `shadow` x `fills` x `exits`) gives the same numbers. With 56
attempts in one quarter hour this is noise (the LEAD itself swings from −0.4% to +6.1% between holdout sets); it
shows the pipeline working, not the filter's value.

**Why only 16% in the bottom tertile.** The store started empty, so every signer was first-seen (`op_has_hist = 0`)
and no signer's funding was known. In the training rows, first-seen signers fall in the bottom tertile only 17% of
the time (45% middle, 38% top); signers with history fill it (56%). So in `skip` mode the filter would drop about
one attempt in six at first, rising towards one in three as the outcome store fills.


## 4. What this does not settle

* The skip rule's lift (+1.4 to +1.5 points) is measured on simulated rank-1 fills. Whether we land as often on the
  kept launches as on the dropped ones is the shadow-mode question of docs/04 §7; the critic's competition counts say
  the kept launches are only modestly more contested.
* The live store starts empty: a first-seen signer gets the training defaults, so the history-driven part of the score
  (AUC 0.77 with history vs 0.67 first-seen on the critic's holdout) builds up over days of running.
* Re-freeze when the collector's full-feed tapes give a week of new launches, with the same freeze-then-holdout
  protocol; the hourly-refitted GBT-3 and ridge cuts of the critic remain to be logged.

## 5. Create-only variant (`entry_filter_v1_create_only`, 2026-10-16)

**Why.** The live hot loop fires before the bundle run behind the create has streamed in (`feed_bundle` is assembled
after the decision), so the 17 bundle-shape features of `entry_filter_v1` (`nB` .. `tok_pct_insiders`) are not
known when it must decide. Only operator history (the outcome store) and the create transaction are.

**What.** The same model (GBT depth 3, P(take-profit), operator pools as training rows and history, identical
hyper-parameters and tertile protocol) on the 44 create-time features: the 43 non-bundle live features (create
transaction, uri host, signer funding, operator history as of the launch, time, create rate) plus `intx_n_buys`
(buy instructions inside the create transaction). The in-transaction buy **size** is not in the training table; it
needs a table rebuild from the tapes (the trades whose signature is the create's) and is the obvious next feature.
Code: `analysis/scripts/strategy/entry_filter_create_only.py` (same `entry_filter.design` / `fit` / `wquant` code
path, `entry_filter.CREATE_ONLY_FEATURES`). Output: `data/reports/strategy/entry_filter_v1_create_only.json`, same
format as v1 (refit on all 3,591 rows, frozen t1 = 0.1575, t2 = 0.2313, in-sample AUC 0.81), and its `.pkl`.

**Holdout (fit on the predictor table only, scored once on the same 480 random HA / HB / HC launches, plain policy
+3.21%; CIs resample signers, 343 clusters):**

| Model | AUC (TP) | Kept | EV kept | Skip-bottom lift [90% CI] | HA / HB / HC lift | Bottom-tertile EV |
|---|---|---|---|---|---|---|
| full v1 features (60) | 0.724 | 62% | +4.73% | **+1.52** [+0.49, +2.54] | +3.06 / +1.30 / +0.09 | +0.77% |
| **create-only (44)** | 0.704 | 68% | +4.78% | **+1.57** [+0.82, +2.37] | +1.54 / +1.88 / −0.35 | −0.14% |
| full model, bundle features blanked at scoring | 0.655 | 21% | +5.43% | +2.22 [−0.39, +4.79] | +0.55 / +4.50 / −3.41 | |

Create-only minus full: **+0.05 points [−0.73, +0.92]** (paired, same launches). The two models keep 271 launches in
common; create-only keeps 56 the full model drops and drops 25 it keeps. Reading: on this holdout the bundle block
adds nothing measurable to the skip-bottom-tertile rule; the skill is in operator history and the create
transaction. The create-only rule is slightly negative on the 59-launch HC set (−0.35), so treat the lift as about
+1.5 points with a CI that reaches +0.8. **Do not score v1 with the bundle block missing**: with zeros in place of the
bundle features it keeps only 21% of launches and its lift is not significant. These holdouts were already used by
the critic (consistency check, as for v1); the first clean test is the shadow run's tertile split, which
`shadow_gate.py` now prints with CIs.

**Look-ahead check of the feature code (done for this variant, applies to v1 too).**
* Operator history (`predictor_build.py`, `angles_eval.py`): `prev` = the signer's launches with `bt < bt(launch)`;
  outcome-based fields (`lead_n`, `lead_hit`, `lead_ev`, `s0t`) use only launches with `bt <= bt(launch) − 10 s`
  (the live store's write delay); `busy` needs `bt <= bt − 900 s`. Point in time, no look-ahead.
* Funding (`predictor.Funding`): gap-1 records are per create signature; window records are used only when the
  signer's first window create is at or before the launch (the 61-row leak fixed in strategy-angles-critic §2.2 is
  re-derived in `entry_filter.table_rows`); `funding_extra.jsonl` records are "one signature page before" the
  signer's first table create and are admitted regardless of time: 1 of 3,591 rows uses one, and its as-of create
  is not later than the launch. Clean in practice.
* Create rate (`CreateIndex.rate`): trailing window `[bt − w, bt)`. Hour, weekday, regime: from the block time.
* Residual, not a feature leak: the operator **pools** (1,491 training/history rows) were listing-sampled after the
  fact (strategy-critic: they overstate armed operators by 4.7 points). They enter `op_all` history only as earlier
  launches of the same signer, so they cannot leak a launch's own outcome, but the pool signers' presence in history
  reflects later knowledge. The holdout rows are random, so the holdout lift is not inflated by this; the live
  store has no pools (it holds only our own outcomes), which is the gap the probe's pre-warm closes
  (`analysis/scripts/prewarm_outcomes.py`).
* Rust side: the hot loop must add `intx_n_buys` to the create-tx feature extraction before it can load this model.
