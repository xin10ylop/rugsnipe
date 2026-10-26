#!/bin/bash
S=/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad
cd $S/oos
xargs -P 4 -I{} python3 -I pull.py {} 1792951200 < cands2.txt
python3 -I mints2.py
python3 -I coin5o.py mints2.txt
echo PULLED
