#!/usr/bin/env bash
# OS tuning for the Frankfurt hot box (fra1), docs/03-speed-infra.md section 7 and
# docs/06-architecture.md section 12.
#
#   sudo deploy/tune-hot.sh --check      # default: only report current vs wanted values
#   sudo deploy/tune-hot.sh --dry-run    # show what --apply would change
#   sudo deploy/tune-hot.sh --apply      # apply the runtime part (idempotent)
#
# Options (or the same names in /etc/rugsnipe/tune-hot.env, used by rugsnipe-tune.service):
#   --iface IF        NIC carrying the feeds (default: the default-route interface)
#   --isolate LIST    CPUs to isolate for rugsnipe-hot (default: /sys/devices/system/cpu/isolated,
#                     else a suggestion: the last 4 physical cores and their SMT siblings)
#   --busy-poll-us N  net.core.busy_poll / busy_read in microseconds (default 50)
#
# Applied at runtime (each step guarded, prints what it changed):
#   1. cpufreq governor = performance on every CPU
#   2. irqbalance stopped and disabled; NIC IRQs pinned to the housekeeping CPUs
#      (away from the isolated busy-poll cores)
#   3. sysctls (persisted in /etc/sysctl.d/90-rugsnipe-hot.conf): busy_poll/busy_read,
#      128 MiB socket buffers for shred bursts, netdev backlog, tcp_autocorking=0,
#      tcp_slow_start_after_idle=0 (warm sender connections), swappiness=1, numa_balancing=0
#   4. transparent hugepages = madvise (enabled and defrag)
# Printed, NEVER applied (kernel command line; needs update-grub + reboot):
#   5. C-state cap (intel_idle.max_cstate=1 / processor.max_cstate=1)
#   6. isolcpus / nohz_full / rcu_nocbs / irqaffinity for the feed, decode/sign, sender and
#      exit threads
set -euo pipefail

MODE=check
IFACE=${IFACE:-}
ISOLATE=${ISOLATE:-}
BUSY_POLL_US=${BUSY_POLL_US:-50}
SYSCTL_FILE=/etc/sysctl.d/90-rugsnipe-hot.conf

while [ $# -gt 0 ]; do
  case "$1" in
    --check) MODE=check; shift ;;
    --dry-run) MODE=dry; shift ;;
    --apply) MODE=apply; shift ;;
    --iface) IFACE=${2:-}; shift 2 ;;
    --isolate) ISOLATE=${2:-}; shift 2 ;;
    --busy-poll-us) BUSY_POLL_US=${2:-}; shift 2 ;;
    -h|--help) sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 64 ;;
  esac
done
case "$ISOLATE" in *[!0-9,-]*) echo "invalid --isolate '$ISOLATE'" >&2; exit 64 ;; esac
case "$BUSY_POLL_US" in ''|*[!0-9]*) echo "invalid --busy-poll-us" >&2; exit 64 ;; esac
case "$IFACE" in *[!A-Za-z0-9._@-]*) echo "invalid --iface '$IFACE'" >&2; exit 64 ;; esac
if [ "$MODE" = apply ] && [ "$(id -u)" -ne 0 ]; then
  echo "--apply needs root" >&2; exit 1
fi

N_DIFF=0
N_SET=0
ok()   { printf '[tune] OK    %s\n' "$*"; }
diff_() { printf '[tune] DIFF  %s\n' "$*"; N_DIFF=$((N_DIFF + 1)); }
set_() { printf '[tune] SET   %s\n' "$*"; N_SET=$((N_SET + 1)); }
skip() { printf '[tune] SKIP  %s\n' "$*"; }
hint() { printf '[tune] HINT  %s\n' "$*"; }

# read_sel FILE: current value; for "always [madvise] never" style files, the bracketed word.
read_sel() {
  local v
  v=$(cat "$1" 2>/dev/null) || return 1
  if [[ $v =~ \[([^]]+)\] ]]; then echo "${BASH_REMATCH[1]}"; else echo "$v"; fi
}

# want_file FILE WANT LABEL: compare / change one sysfs or procfs value.
want_file() {
  local f=$1 want=$2 label=$3 cur
  if [ ! -e "$f" ]; then skip "$label: $f not available (VM or kernel without it)"; return 0; fi
  cur=$(read_sel "$f" | tr -s '[:space:]' ' ' | sed 's/ $//') || { skip "$label: unreadable"; return 0; }
  if [ "$cur" = "$want" ]; then ok "$label = $cur"; return 0; fi
  case "$MODE" in
    check) diff_ "$label = $cur (want $want)" ;;
    dry) diff_ "$label: would set $cur -> $want" ;;
    apply)
      if printf '%s' "$want" > "$f" 2>/dev/null; then set_ "$label: $cur -> $want"
      else skip "$label: write refused ($cur stays; read-only in this VM?)"; fi ;;
  esac
}

expand_cpus() {  # "0-3,8" -> "0 1 2 3 8"
  local part a b i parts out=()
  IFS=, read -r -a parts <<< "$1"
  for part in "${parts[@]}"; do
    [ -n "$part" ] || continue
    a=${part%-*}; b=${part#*-}
    for ((i = a; i <= b; i++)); do out+=("$i"); done
  done
  echo "${out[@]}"
}
cpus_minus() {  # ALL SUB -> comma list
  local c s keep out=()
  for c in $(expand_cpus "$1"); do
    keep=1
    for s in $(expand_cpus "$2"); do [ "$c" = "$s" ] && keep=0; done
    [ "$keep" = 1 ] && out+=("$c")
  done
  local IFS=,
  echo "${out[*]}"
}

ONLINE=$(cat /sys/devices/system/cpu/online 2>/dev/null || echo "0-$(( $(nproc) - 1 ))")
ISOLATED_NOW=$(cat /sys/devices/system/cpu/isolated 2>/dev/null || true)

# suggest_isolate: last 4 physical cores (2 if < 8 cores) plus their SMT siblings.
suggest_isolate() {
  local c core_ids=() n take sel=() sib
  for c in $(expand_cpus "$ONLINE"); do
    [ "$c" = 0 ] && continue
    sib=$(cat "/sys/devices/system/cpu/cpu$c/topology/thread_siblings_list" 2>/dev/null || echo "$c")
    # first thread of each physical core represents it
    [ "${sib%%[,-]*}" = "$c" ] && core_ids+=("$c")
  done
  n=${#core_ids[@]}
  take=4; [ "$n" -lt 8 ] && take=2
  [ "$n" -le "$take" ] && { echo ""; return 0; }
  for c in "${core_ids[@]:$((n - take))}"; do
    sel+=("$(cat "/sys/devices/system/cpu/cpu$c/topology/thread_siblings_list" 2>/dev/null || echo "$c")")
  done
  local IFS=,
  echo "${sel[*]}"
}

[ -n "$ISOLATE" ] || ISOLATE=$ISOLATED_NOW
[ -n "$ISOLATE" ] || ISOLATE=$(suggest_isolate)
HK=""
[ -n "$ISOLATE" ] && HK=$(cpus_minus "$ONLINE" "$ISOLATE")

echo "[tune] mode=$MODE kernel=$(uname -r) cpus online=$ONLINE isolated(now)=${ISOLATED_NOW:-none} target isolate=${ISOLATE:-none} housekeeping=${HK:-all}"

# ---------------------------------------------------------------- 1. governor
n_cpu=0; n_ok=0; govs=""
for g in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor; do
  [ -e "$g" ] || continue
  n_cpu=$((n_cpu + 1))
  cur=$(cat "$g")
  if [ "$cur" = performance ]; then n_ok=$((n_ok + 1)); continue; fi
  govs="$cur"
  if [ "$MODE" = apply ]; then
    printf performance > "$g" 2>/dev/null && n_ok=$((n_ok + 1)) || true
  fi
done
if [ "$n_cpu" = 0 ]; then
  skip "governor: no cpufreq (VM; the hypervisor owns frequency)"
elif [ "$n_ok" = "$n_cpu" ] && [ -z "$govs" ]; then
  ok "governor = performance ($n_cpu/$n_cpu cpus)"
elif [ "$MODE" = apply ]; then
  set_ "governor: $govs -> performance ($n_ok/$n_cpu cpus)"
else
  diff_ "governor = $govs on $((n_cpu - n_ok))/$n_cpu cpus (want performance)"
fi
want_file /sys/devices/system/cpu/cpufreq/boost 1 "cpufreq boost"

# ---------------------------------------------------------------- 2. irqbalance + IRQ pinning
if command -v systemctl >/dev/null && [ -d /run/systemd/system ]; then
  if systemctl is-active --quiet irqbalance 2>/dev/null || systemctl is-enabled --quiet irqbalance 2>/dev/null; then
    case "$MODE" in
      apply) systemctl disable --now --no-block irqbalance >/dev/null 2>&1 && set_ "irqbalance: stopped and disabled" ;;
      *) diff_ "irqbalance is active/enabled (want stopped + disabled)" ;;
    esac
  else
    ok "irqbalance not running"
  fi
else
  skip "irqbalance: systemd not running here"
fi

if [ -z "$IFACE" ] && [ -r /proc/net/route ]; then
  IFACE=$(awk '$2 == "00000000" { print $1; exit }' /proc/net/route)
fi
pin_irqs() {  # pin_irqs IFACE CPULIST
  local ifc=$1 cpus=$2 irq f cur n=0 nd=0
  local d=/sys/class/net/$ifc/device/msi_irqs
  if [ ! -d "$d" ]; then skip "irq pinning: $ifc has no MSI IRQs (virtio/ENA without MSI-X?)"; return 0; fi
  for irq in "$d"/*; do
    irq=${irq##*/}
    f=/proc/irq/$irq/smp_affinity_list
    [ -e "$f" ] || continue
    cur=$(cat "$f")
    [ "$cur" = "$cpus" ] && continue
    nd=$((nd + 1))
    if [ "$MODE" = apply ]; then
      printf '%s' "$cpus" > "$f" 2>/dev/null && n=$((n + 1)) || true
    fi
  done
  if [ "$nd" = 0 ]; then ok "irq pinning: all $ifc IRQs on housekeeping CPUs $cpus"
  elif [ "$MODE" = apply ]; then set_ "irq pinning: $n/$nd $ifc IRQs -> CPUs $cpus"
  else diff_ "irq pinning: $nd $ifc IRQs not on housekeeping CPUs $cpus"; fi
}
if [ -z "$IFACE" ]; then
  skip "irq pinning: no default-route interface found (pass --iface)"
elif [ -z "$HK" ]; then
  skip "irq pinning: no isolated CPU set, nothing to keep IRQs away from"
else
  pin_irqs "$IFACE" "$HK"
fi

# ---------------------------------------------------------------- 3. sysctls
SYSCTLS=(
  "net.core.busy_poll=$BUSY_POLL_US"
  "net.core.busy_read=$BUSY_POLL_US"
  "net.core.rmem_max=134217728"
  "net.core.rmem_default=134217728"
  "net.core.wmem_max=134217728"
  "net.core.wmem_default=134217728"
  "net.core.netdev_max_backlog=30000"
  "net.ipv4.tcp_autocorking=0"
  "net.ipv4.tcp_slow_start_after_idle=0"
  "vm.swappiness=1"
  "kernel.numa_balancing=0"
)
for kv in "${SYSCTLS[@]}"; do
  k=${kv%%=*}; v=${kv#*=}
  want_file "/proc/sys/${k//.//}" "$v" "sysctl $k"
done
persist=$(printf '# rugsnipe hot box (deploy/tune-hot.sh). docs/03-speed-infra.md section 7.\n'; printf '%s\n' "${SYSCTLS[@]}" | sed 's/=/ = /')
if [ -f "$SYSCTL_FILE" ] && [ "$(cat "$SYSCTL_FILE")" = "$persist" ]; then
  ok "persisted in $SYSCTL_FILE"
else
  case "$MODE" in
    apply) printf '%s\n' "$persist" > "$SYSCTL_FILE" && set_ "wrote $SYSCTL_FILE (survives reboot)" ;;
    *) diff_ "$SYSCTL_FILE missing or different (written by --apply)" ;;
  esac
fi

# ---------------------------------------------------------------- 4. THP
want_file /sys/kernel/mm/transparent_hugepage/enabled madvise "THP enabled"
want_file /sys/kernel/mm/transparent_hugepage/defrag madvise "THP defrag"

# ---------------------------------------------------------------- 5. C-states (hint only)
vendor=$(awk -F: '/^vendor_id/ { gsub(/ /, "", $2); print $2; exit }' /proc/cpuinfo 2>/dev/null || true)
CMDLINE=$(cat /proc/cmdline 2>/dev/null || true)
deep=""
for s in /sys/devices/system/cpu/cpu0/cpuidle/state[0-9]*; do
  [ -e "$s/name" ] || continue
  deep="$deep $(cat "$s/name")($(cat "$s/latency")us$( [ "$(cat "$s/disable" 2>/dev/null)" = 1 ] && echo ',off'))"
done
if [ -n "$deep" ]; then echo "[tune] INFO  cpuidle states on cpu0:$deep"; else skip "cpuidle: no states exposed (VM)"; fi
case "$vendor" in
  GenuineIntel) CST="intel_idle.max_cstate=1 processor.max_cstate=1" ;;
  AuthenticAMD) CST="processor.max_cstate=1" ;;  # plus BIOS: Global C-state Control = Disabled
  *) CST="processor.max_cstate=1" ;;
esac
if [[ $CMDLINE == *max_cstate=1* ]]; then ok "C-states capped on the kernel cmdline"
else hint "C-state cap (not applied): add '$CST' to the kernel cmdline (also set BIOS: C-states off / Power profile = performance)"; fi

# ---------------------------------------------------------------- 6. isolcpus grub hint
if [ -n "$ISOLATE" ]; then
  GRUB_ADD="isolcpus=managed_irq,domain,$ISOLATE nohz_full=$ISOLATE rcu_nocbs=$ISOLATE irqaffinity=$HK $CST"
  if [[ $CMDLINE == *isolcpus=* ]] && [[ $CMDLINE == *nohz_full=* ]] && [[ $CMDLINE == *rcu_nocbs=* ]]; then
    ok "isolcpus/nohz_full/rcu_nocbs present on the kernel cmdline (isolated: ${ISOLATED_NOW:-?})"
  else
    hint "kernel cmdline (not applied). In /etc/default/grub append to GRUB_CMDLINE_LINUX_DEFAULT:"
    hint "    $GRUB_ADD"
    hint "then: sudo update-grub && sudo reboot; afterwards re-run deploy/install.sh --role hot (pins rugsnipe-hot to $ISOLATE)"
  fi
else
  skip "isolcpus: too few cores to isolate any (need >= 4 physical cores)"
fi

# ---------------------------------------------------------------- reports only
if command -v ethtool >/dev/null && [ -n "$IFACE" ]; then
  co=$(ethtool -c "$IFACE" 2>/dev/null | awk '/^(Adaptive RX|rx-usecs):/ { printf "%s ", $0 }')
  [ -n "$co" ] && echo "[tune] INFO  $IFACE coalescing: $co(consider: ethtool -C $IFACE adaptive-rx off rx-usecs 0)"
fi
if command -v chronyc >/dev/null; then
  off=$(chronyc -n tracking 2>/dev/null | awk -F': ' '/^System time/ { print $2 }')
  src=$(chronyc -n tracking 2>/dev/null | awk -F': ' '/^Reference ID/ { print $2 }')
  [ -n "$off" ] && echo "[tune] INFO  chrony: $off (ref $src); rugsnipe refuses probe/live above 5 ms"
fi

echo "[tune] done: mode=$MODE, $N_SET change(s) made, $N_DIFF item(s) differ"
