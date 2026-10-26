# The dump-and-bounce test (docs/19), Oct 26

Run from the session scratchpad. `scan.py DROP RISE` (and `scan2.py ... oos`): dumps in the first 13 slots of every launch
with a complete tape (a sell taking >= DROP off the high after a rise >= RISE). `paths.py EVENTS [WIN] [B]`: the curve
every B slots for WIN slots after each dump (the last landed trade of each bucket) -> `SP/flush/path/`. `bounce.py`:
entries (right after, after a confirmed rebound, on a deeper fall) and exits (take profit, stop, time limit) by day and
seller. `later.py`: the same on later dumps inside those paths (a run-up, then a flush).
