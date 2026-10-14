#!/usr/bin/env bash
# Measure round-trip latency from THIS machine to the endpoints that matter for a Solana sniper.
# Run it on every candidate server (e.g. your AWS Ohio box) and compare:
#   bash deploy/latency_probe.sh            # 10 samples per host
#   bash deploy/latency_probe.sh 30         # 30 samples per host
# Output: median / min TCP connect time in ms (TLS handshake excluded), lower is better.
# Jito block engines: the region whose block engine answers fastest is the one to tip through.
# Helius Sender / LaserStream regions and the public RPCs are included for reference.
set -u
N=${1:-10}

HOSTS=(
  # Jito block engines (bundles + shredstream auth) -----------------------------
  "jito-ny          ny.mainnet.block-engine.jito.wtf:443"
  "jito-amsterdam   amsterdam.mainnet.block-engine.jito.wtf:443"
  "jito-frankfurt   frankfurt.mainnet.block-engine.jito.wtf:443"
  "jito-london      london.mainnet.block-engine.jito.wtf:443"
  "jito-dublin      dublin.mainnet.block-engine.jito.wtf:443"
  "jito-tokyo       tokyo.mainnet.block-engine.jito.wtf:443"
  "jito-singapore   singapore.mainnet.block-engine.jito.wtf:443"
  "jito-slc         slc.mainnet.block-engine.jito.wtf:443"
  "jito-global      mainnet.block-engine.jito.wtf:443"
  # Helius Sender regional endpoints (http) --------------------------------------
  "helius-slc       slc-sender.helius-rpc.com:443"
  "helius-ewr       ewr-sender.helius-rpc.com:443"
  "helius-lon       lon-sender.helius-rpc.com:443"
  "helius-fra       fra-sender.helius-rpc.com:443"
  "helius-ams       ams-sender.helius-rpc.com:443"
  "helius-sg        sg-sender.helius-rpc.com:443"
  "helius-tyo       tyo-sender.helius-rpc.com:443"
  "helius-global    sender.helius-rpc.com:443"
  # Public RPCs and pump.fun API (reference) --------------------------------------
  "solana-public    api.mainnet-beta.solana.com:443"
  "publicnode       solana-rpc.publicnode.com:443"
  "pumpfun-api      frontend-api-v3.pump.fun:443"
  "helius-rpc       mainnet.helius-rpc.com:443"
)

have_curl=$(command -v curl || true)
if [ -z "$have_curl" ]; then echo "curl is required"; exit 1; fi

printf "%-18s %-48s %8s %8s %8s %6s\n" "name" "host" "min_ms" "med_ms" "max_ms" "ok"
for entry in "${HOSTS[@]}"; do
  name=$(echo "$entry" | awk '{print $1}')
  host=$(echo "$entry" | awk '{print $2}')
  times=()
  ok=0
  for i in $(seq 1 "$N"); do
    # time_connect = TCP connect only; resolves DNS once per call (first call warms the cache)
    t=$(curl -s -o /dev/null --max-time 3 -w "%{time_connect}" "https://$host/" 2>/dev/null || echo "")
    if [ -n "$t" ] && [ "$t" != "0.000000" ]; then
      times+=("$t"); ok=$((ok+1))
    fi
  done
  if [ "$ok" -eq 0 ]; then
    printf "%-18s %-48s %8s %8s %8s %6s\n" "$name" "$host" "-" "-" "-" "0/$N"
    continue
  fi
  sorted=$(printf "%s\n" "${times[@]}" | sort -n)
  min=$(echo "$sorted" | head -1)
  max=$(echo "$sorted" | tail -1)
  med=$(echo "$sorted" | awk '{a[NR]=$1} END{ if (NR%2==1) print a[(NR+1)/2]; else print (a[NR/2]+a[NR/2+1])/2 }')
  printf "%-18s %-48s %8.1f %8.1f %8.1f %6s\n" "$name" "$host" "$(echo "$min*1000" | bc -l)" "$(echo "$med*1000" | bc -l)" "$(echo "$max*1000" | bc -l)" "$ok/$N"
done

echo
echo "Interpretation: for block-0 sniping what matters is RTT to the CURRENT slot leader, which rotates"
echo "every 4 slots across validators worldwide; the Jito/Helius regional endpoints are a good proxy for"
echo "where the validator hubs are (NY/Secaucus, Amsterdam, Frankfurt, Tokyo, SLC). A region where several"
echo "hubs are < 10 ms away (e.g. Frankfurt/Amsterdam for the EU hubs) beats one that is 20-40 ms from all."
