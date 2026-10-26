# The 3yrJE1A8 copy's exit (docs/19 section 5), Oct 26

`grid.py`: for each coin 3yrJE1A8 bought since Oct 24 (its own transactions, `SP/hold/log/3yrJE1A8.jsonl`), the curve state
at its buy slot + 5 / 9 / 14 / 17 / 19 / 24 / 29 (the last landed trade at or before each). `eval.py`: the copy (our
landing from the complete tapes, else its post-buy price + 2%; refused above +5%; 0.5 SOL, fees, 3M + 0.5M tips) sold at
each, by day and by its own timer (20 or 30 slots), and the variant that sells right after its first sell. A trade event
can report a 0 SOL reserve; the state is then rebuilt from the token reserve.
