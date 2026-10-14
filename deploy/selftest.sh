#!/usr/bin/env bash
# ok() never fails, so "A && ok || bad" is a safe if-then-else here
# shellcheck disable=SC2015
# Offline self test of the deployment layer. Changes nothing outside a temp directory.
#
#   bash deploy/selftest.sh
#
# Checks: bash -n and shellcheck (if installed; set SHELLCHECK=/path/to/shellcheck) on every
# script; systemd-analyze verify + offline security score on every unit (in a scratch root
# with stub binaries); unit invariants (Type=notify, WatchdogSec, credentials, hardening,
# CAP_SYS_NICE only on hot); the example config parses and keeps mode = paper; install.sh
# --dry-run for both roles; install.sh refuses group/world-readable or role-forbidden keys;
# tune-hot.sh --check / --dry-run; no key material committed under deploy/ or config/.
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
PASS=0
FAIL=0
ok()   { printf 'ok    %s\n' "$*"; PASS=$((PASS + 1)); }
bad()  { printf 'FAIL  %s\n' "$*"; FAIL=$((FAIL + 1)); }
skip() { printf 'skip  %s\n' "$*"; }
check() { local name=$1; shift; if "$@" >"$TMP/out" 2>&1; then ok "$name"; else bad "$name"; sed 's/^/      /' "$TMP/out" | tail -20; fi; }

SCRIPTS=("$HERE/install.sh" "$HERE/tune-hot.sh" "$HERE/selftest.sh" "$HERE/bin/rugsnipe-pyrun" "$HERE/latency_probe.sh")
UNITS=("$HERE"/systemd/*.service "$HERE"/systemd/*.timer "$HERE"/systemd/*.target)

# ---------------------------------------------------------------- syntax + lint
for s in "${SCRIPTS[@]}"; do check "bash -n ${s#"$REPO"/}" bash -n "$s"; done
SC=${SHELLCHECK:-$(command -v shellcheck || true)}
if [ -n "$SC" ]; then
  # latency_probe.sh predates this layer and is only syntax-checked
  for s in "${SCRIPTS[@]:0:4}"; do check "shellcheck ${s#"$REPO"/}" "$SC" -x "$s"; done
else
  skip "shellcheck not installed (pip install shellcheck-py into a scratch venv, then SHELLCHECK=...)"
fi

# ---------------------------------------------------------------- unit invariants
has() { grep -Eq "$2" "$1"; }
for u in rugsnipe-hot rugsnipe-guard rugsnipe-keeper; do
  f=$HERE/systemd/$u.service
  for pat in '^Type=notify$' '^WatchdogSec=[0-9]+' '^LoadCredential=hot-keypair:' '^SetCredential=hot-keypair:ABSENT$' \
             '^ProtectSystem=strict$' '^NoNewPrivileges=yes$' '^Restart=always$' '^User=rugsnipe$' \
             '^ReadWritePaths=/var/lib/rugsnipe /run/rugsnipe$' '^CPUAffinity='; do
    has "$f" "$pat" || bad "$u.service lacks $pat"
  done
done
has "$HERE/systemd/rugsnipe-hot.service" '^CapabilityBoundingSet=CAP_SYS_NICE$' && ok "hot keeps CAP_SYS_NICE only" || bad "hot CapabilityBoundingSet"
has "$HERE/systemd/rugsnipe-hot.service" '^WatchdogSec=3$' && ok "hot WatchdogSec=3 (docs/06 s12)" || bad "hot WatchdogSec"
for f in "$HERE"/systemd/*.service; do
  case "$f" in */rugsnipe-hot.service|*/rugsnipe-tune.service) continue ;; esac
  has "$f" '^CapabilityBoundingSet=$' || bad "$(basename "$f") must have an empty CapabilityBoundingSet"
done
ok "unit invariants checked"
if grep -h '^LoadCredential=' "$HERE"/systemd/*.service | grep -v ':/etc/rugsnipe/keys/' | grep -q .; then
  bad "a LoadCredential points outside /etc/rugsnipe/keys"
else
  ok "all credentials load from /etc/rugsnipe/keys"
fi

# ---------------------------------------------------------------- systemd-analyze
if command -v systemd-analyze >/dev/null && [ -d /usr/lib/systemd/system ]; then
  R=$TMP/root
  mkdir -p "$R/opt/rugsnipe/bin" "$R/etc/systemd/system" "$R/usr/lib/systemd"
  cp -a /usr/lib/systemd/system "$R/usr/lib/systemd/"
  for b in rugsnipe-hot rugsnipe-guard rugsnipe-keeper rugsnipe-pyrun tune-hot.sh; do
    printf '#!/bin/sh\n' > "$R/opt/rugsnipe/bin/$b"; chmod +x "$R/opt/rugsnipe/bin/$b"
  done
  cp "${UNITS[@]}" "$R/etc/systemd/system/"
  names=(); for u in "${UNITS[@]}"; do names+=("$(basename "$u")"); done
  (cd "$R/etc/systemd/system" && systemd-analyze verify --root="$R" "${names[@]}") >"$TMP/out" 2>&1
  if [ ! -s "$TMP/out" ]; then ok "systemd-analyze verify (${#names[@]} units)"; else bad "systemd-analyze verify"; sed 's/^/      /' "$TMP/out"; fi
  for u in rugsnipe-hot rugsnipe-guard rugsnipe-keeper rugsnipe-collector rugsnipe-discover; do
    score=$(systemd-analyze security --offline=true --root="$R" "$u.service" 2>/dev/null | awk '/Overall exposure level/ { print $(NF-2) }')
    if [ -n "$score" ] && awk -v s="$score" 'BEGIN { exit !(s <= 3.0) }'; then ok "security $u: $score"
    else bad "security $u: ${score:-n/a} (want <= 3.0)"; fi
  done
else
  skip "systemd-analyze not available"
fi

# ---------------------------------------------------------------- example config
PY=$(command -v python3 || true)
if [ -n "$PY" ]; then
  check "config/rugsnipe.example.toml parses, mode=paper, rpc_budget <= 32" "$PY" -I -c '
import sys, tomllib
d = tomllib.load(open(sys.argv[1], "rb"))
assert d["mode"]["mode"] == "paper", d["mode"]
assert sum(d["rpc_budget"].values()) <= 32
assert d["risk"]["kill_file"] == "/run/rugsnipe/KILL"
assert d["obs"]["rslog_dir"] == "/var/lib/rugsnipe/rslog"
for k in ("hot_keypair", "probe_keypair", "api_keys"):
    assert d["secrets"][k].replace("-", "").isalnum(), k   # credential names, not key material
' "$REPO/config/rugsnipe.example.toml"
fi

# ---------------------------------------------------------------- install.sh dry runs
for role in analytics hot; do
  if bash "$HERE/install.sh" --role "$role" --dry-run --no-build >"$TMP/dry-$role" 2>&1 \
     && grep -q 'DRY RUN: nothing changed' "$TMP/dry-$role"; then
    ok "install.sh --role $role --dry-run"
  else
    bad "install.sh --role $role --dry-run"; tail -20 "$TMP/dry-$role" | sed 's/^/      /'
  fi
done
grep -q 'rugsnipe-collector.service' "$TMP/dry-analytics" && ! grep -q 'rugsnipe-hot.service (0644' "$TMP/dry-analytics" \
  && ok "analytics role installs collector, not hot" || bad "analytics unit set"
grep -q 'ntp.frankfurt.jito.wtf iburst prefer' "$TMP/dry-hot" && ok "hot role: chrony -> Jito Frankfurt NTP" || bad "hot chrony"
grep -q '20-cpus.conf' "$TMP/dry-hot" && ok "hot role: CPU pinning drop-ins" || bad "hot cpu drop-ins"
check "install.sh rejects a bad role" bash -c "! bash '$HERE/install.sh' --role nope --dry-run"

# ---------------------------------------------------------------- key refusal
key_case() {  # key_case NAME ROLE FILE MODE EXPECT(refuse|accept)
  local name=$1 role=$2 file=$3 mode=$4 expect=$5 etc=$TMP/etc-$1 rc
  mkdir -p "$etc/keys"; chmod 700 "$etc/keys"
  printf '[1,2,3]' > "$etc/keys/$file"; chmod "$mode" "$etc/keys/$file"
  RUGSNIPE_ETC=$etc bash "$HERE/install.sh" --role "$role" --dry-run --no-build --no-apt >"$TMP/key-$name" 2>&1
  rc=$?
  if { [ "$expect" = refuse ] && [ "$rc" -ne 0 ] && grep -q ERROR "$TMP/key-$name"; } \
     || { [ "$expect" = accept ] && [ "$rc" -eq 0 ]; }; then
    ok "keys: $name -> $expect"
  else
    bad "keys: $name (rc=$rc, want $expect)"; tail -5 "$TMP/key-$name" | sed 's/^/      /'
  fi
}
key_case world-readable hot hot-keypair.json 0644 refuse
key_case group-readable hot hot-keypair.json 0440 refuse
key_case signing-key-on-hot hot armset-signing.key 0400 refuse
key_case wallet-on-analytics analytics hot-keypair.json 0400 refuse
if [ "$(id -u)" -eq 0 ]; then
  key_case root-0400 hot hot-keypair.json 0400 accept
else
  key_case non-root-owner hot hot-keypair.json 0400 refuse
fi

# ---------------------------------------------------------------- tune-hot.sh
check "tune-hot.sh --check" bash "$HERE/tune-hot.sh" --check
check "tune-hot.sh --dry-run" bash "$HERE/tune-hot.sh" --dry-run
check "tune-hot.sh rejects bad --isolate" bash -c "! bash '$HERE/tune-hot.sh' --isolate 'x;y'"

# ---------------------------------------------------------------- no secrets committed
if grep -rEl '\[ *([0-9]{1,3} *, *){63}[0-9]{1,3} *\]|PRIVATE KEY-----' "$HERE" "$REPO/config" 2>/dev/null | grep -v selftest.sh | grep -q .; then
  bad "something that looks like key material is under deploy/ or config/"
else
  ok "no keypair arrays or PEM keys under deploy/ or config/"
fi

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
