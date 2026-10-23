#!/usr/bin/env python3
"""Regenerate data/leader_regions.json from Stakewiz's validator list.

    python3 analysis/scripts/leader_regions.py [--from stakewiz.json] [--out data/leader_regions.json]

The hot box's leader filter (`entry.leader_regions`) skips creates whose slot
leader is outside the listed regions. Regions: eu, na, asia, other, unknown.
"""
import argparse, json, sys, urllib.request

EU = {'Germany', 'Netherlands', 'France', 'United Kingdom', 'Finland', 'Poland', 'Switzerland', 'Ireland', 'Spain',
      'Italy', 'Sweden', 'Austria', 'Belgium', 'Czechia', 'Czech Republic', 'Lithuania', 'Republic of Lithuania',
      'Latvia', 'Norway', 'Denmark', 'Ukraine', 'Russia', 'Russian Federation', 'Romania', 'Bulgaria', 'Hungary',
      'Portugal', 'Estonia', 'Luxembourg', 'Slovakia', 'Slovenia', 'Croatia', 'Serbia', 'Greece', 'Türkiye', 'Turkey',
      'Moldova', 'Belarus', 'Iceland', 'Albania'}
NA = {'United States', 'Canada', 'Mexico'}
ASIA = {'Japan', 'Singapore', 'Hong Kong', 'South Korea', 'India', 'Australia', 'Taiwan', 'China', 'Indonesia',
        'Malaysia', 'Thailand', 'Vietnam', 'Philippines', 'New Zealand', 'United Arab Emirates', 'Israel', 'Kazakhstan'}


def region(country: str) -> str:
    if country in EU:
        return 'eu'
    if country in NA:
        return 'na'
    if country in ASIA:
        return 'asia'
    return 'other' if country else 'unknown'


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument('--from', dest='src', default='', help='a saved Stakewiz validators JSON (default: fetch)')
    ap.add_argument('--out', default='data/leader_regions.json')
    a = ap.parse_args()
    if a.src:
        v = json.load(open(a.src))
    else:
        with urllib.request.urlopen('https://api.stakewiz.com/validators', timeout=60) as r:
            v = json.loads(r.read())
    regions = {x['identity']: region(x.get('ip_country') or '') for x in v if x.get('identity')}
    counts = {}
    for r in regions.values():
        counts[r] = counts.get(r, 0) + 1
    json.dump({'source': 'stakewiz.com /validators; regenerate with analysis/scripts/leader_regions.py', 'regions': regions},
              open(a.out, 'w'), indent=0, sort_keys=True)
    print(f'{a.out}: {len(regions)} validators {counts}', file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main())
