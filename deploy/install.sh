#!/usr/bin/env bash
# rugsnipe installer for Ubuntu 22.04 / 24.04 (docs/06-architecture.md sections 2, 6, 12, 13;
# docs/07-deploy.md). Idempotent: re-run it after every `git pull`.
#
#   sudo deploy/install.sh --role analytics            # ohio1: collector, discover timer
#   sudo deploy/install.sh --role hot                  # fra1: hot, guard, keeper (+ tune unit)
#   deploy/install.sh --role hot --dry-run             # print every action, change nothing
#
# Options:
#   --role analytics|hot   required
#   --user NAME            service user (default rugsnipe)
#   --prefix DIR           install prefix (default /opt/rugsnipe)
#   --ntp-region NAME      Jito NTP region for chrony on the hot role (default frankfurt;
#                          amsterdam, london, dublin, ny, slc, tokyo, singapore)
#   --hot-cpus LIST        CPUs for rugsnipe-hot (default: /sys/devices/system/cpu/isolated)
#   --no-build             skip rustup + cargo build
#   --no-apt               skip apt-get (packages must already be present)
#   --enable               also `systemctl enable` the role's units (never starts them)
#   --dry-run | --check    print what would be done; change nothing; runs without root
#   --force                continue on a non-Ubuntu or unknown Ubuntu release
#
# What it does: `git config core.hooksPath .githooks` in the checkout (pre-commit hook that
# refuses key material); apt packages; system user; rustup with the toolchain pinned in
# rust-toolchain.toml; `cargo build --release --locked` (if the workspace exists); a Python
# venv with the analysis package; chrony (Jito regional NTP on hot, Amazon Time Sync on EC2);
# /etc/rugsnipe (0750) with rugsnipe.toml from config/rugsnipe.example.toml (never
# overwritten); /var/lib/rugsnipe/{rslog,state,armsets,data}; /run/rugsnipe via tmpfiles.d;
# systemd units from deploy/systemd/. It never creates, copies or prints key material and
# REFUSES to continue if anything under /etc/rugsnipe/keys is group/world accessible.
set -euo pipefail

# ------------------------------------------------------------------------------- arguments
ROLE=""
SVC_USER="rugsnipe"
PREFIX="/opt/rugsnipe"
NTP_REGION="frankfurt"
HOT_CPUS=""
DO_BUILD=1
DO_APT=1
DO_ENABLE=0
DRY_RUN=0
FORCE=0

usage() { sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --role) ROLE=${2:-}; shift 2 ;;
    --role=*) ROLE=${1#*=}; shift ;;
    --user) SVC_USER=${2:-}; shift 2 ;;
    --user=*) SVC_USER=${1#*=}; shift ;;
    --prefix) PREFIX=${2:-}; shift 2 ;;
    --prefix=*) PREFIX=${1#*=}; shift ;;
    --ntp-region) NTP_REGION=${2:-}; shift 2 ;;
    --ntp-region=*) NTP_REGION=${1#*=}; shift ;;
    --hot-cpus) HOT_CPUS=${2:-}; shift 2 ;;
    --hot-cpus=*) HOT_CPUS=${1#*=}; shift ;;
    --no-build) DO_BUILD=0; shift ;;
    --no-apt) DO_APT=0; shift ;;
    --enable) DO_ENABLE=1; shift ;;
    --dry-run|--check) DRY_RUN=1; shift ;;
    --force) FORCE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 64 ;;
  esac
done

case "$ROLE" in analytics|hot) ;; *) echo "--role analytics|hot is required" >&2; exit 64 ;; esac
case "$SVC_USER" in ''|*[!a-z0-9_-]*) echo "invalid --user '$SVC_USER'" >&2; exit 64 ;; esac
case "$NTP_REGION" in
  frankfurt|amsterdam|london|dublin|ny|slc|tokyo|singapore) ;;
  *) echo "invalid --ntp-region '$NTP_REGION'" >&2; exit 64 ;;
esac
case "$HOT_CPUS" in *[!0-9,-]*) echo "invalid --hot-cpus '$HOT_CPUS'" >&2; exit 64 ;; esac
case "$PREFIX" in /*) ;; *) echo "--prefix must be absolute" >&2; exit 64 ;; esac

# ------------------------------------------------------------------------------- constants
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SRC="$PREFIX/src"
VENV="$PREFIX/venv"
BIN="$PREFIX/bin"
ETC=${RUGSNIPE_ETC:-/etc/rugsnipe}   # override only for deploy/selftest.sh
KEYS=$ETC/keys
STATE=/var/lib/rugsnipe
UNIT_DIR=/etc/systemd/system
RUSTUP_HOME_DIR="$PREFIX/rustup"
CARGO_HOME_DIR="$PREFIX/cargo"
TARGET_DIR="$PREFIX/target"
RUST_BINS=(rugsnipe-hot rugsnipe-guard rugsnipe-keeper rugsnipe-ctl rugsnipe-replay rugsnipe-sim)

if [ "$ROLE" = hot ]; then
  UNITS=(rugsnipe-keeper.service rugsnipe-guard.service rugsnipe-hot.service rugsnipe-tune.service)
  ENABLE_UNITS=(rugsnipe-keeper.service rugsnipe-guard.service rugsnipe-hot.service rugsnipe.target)
else
  UNITS=(rugsnipe-collector.service rugsnipe-discover.service rugsnipe-discover.timer)
  ENABLE_UNITS=(rugsnipe-collector.service rugsnipe-discover.timer rugsnipe.target)
fi

# ------------------------------------------------------------------------------- helpers
log()  { printf '[install] %s\n' "$*"; }
warn() { printf '[install] WARNING: %s\n' "$*" >&2; }
die()  { printf '[install] ERROR: %s\n' "$*" >&2; exit 1; }

# run CMD...: execute, or print it in dry-run mode.
run() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '[dry-run] %s\n' "$*"
  else
    "$@"
  fi
}

# put_file DEST MODE OWNER:GROUP  (content on stdin): write only if content/perms differ.
put_file() {
  local dest=$1 mode=$2 owner=$3 tmp
  tmp=$(mktemp)
  cat > "$tmp"
  if [ "$DRY_RUN" = 1 ]; then
    if [ -f "$dest" ] && cmp -s "$tmp" "$dest"; then
      printf '[dry-run] %s unchanged\n' "$dest"
    else
      printf '[dry-run] write %s (%s %s):\n' "$dest" "$mode" "$owner"
      head -n 25 "$tmp" | sed 's/^/[dry-run]   | /'
      n=$(wc -l < "$tmp")
      [ "$n" -le 25 ] || printf '[dry-run]   | ... (%s lines total)\n' "$n"
    fi
    rm -f "$tmp"
    return 0
  fi
  if [ -f "$dest" ] && cmp -s "$tmp" "$dest"; then
    rm -f "$tmp"
  else
    install -D -m "$mode" -o "${owner%%:*}" -g "${owner##*:}" "$tmp" "$dest"
    rm -f "$tmp"
    log "wrote $dest"
  fi
  chmod "$mode" "$dest"
  chown "$owner" "$dest"
}

# ensure_dir PATH MODE OWNER:GROUP
ensure_dir() {
  local path=$1 mode=$2 owner=$3
  if [ "$DRY_RUN" = 1 ]; then
    if [ -d "$path" ]; then
      printf '[dry-run] dir %s exists (%s); would set %s %s\n' "$path" \
        "$(stat -c '%a %U:%G' "$path")" "$mode" "$owner"
    else
      printf '[dry-run] mkdir %s (%s %s)\n' "$path" "$mode" "$owner"
    fi
    return 0
  fi
  install -d -m "$mode" -o "${owner%%:*}" -g "${owner##*:}" "$path"
  chmod "$mode" "$path"
  chown "$owner" "$path"
}

# check_keys: refuse if any key file/dir is group- or world-accessible, or role-forbidden.
check_keys() {
  [ -d "$KEYS" ] || { log "keys: $KEYS does not exist yet (fine for paper mode)"; return 0; }
  local bad dperm
  dperm=$(stat -c '%a' "$KEYS")
  if [ "$((8#$dperm & 8#077))" -ne 0 ]; then
    die "$KEYS is mode $dperm; it must be 0700 root:root. Fix: chmod 700 $KEYS"
  fi
  bad=$(find "$KEYS" -mindepth 1 \( -perm /g=rwx,o=rwx -o ! -user root \) -printf '%m %u %p\n')
  if [ -n "$bad" ]; then
    printf '%s\n' "$bad" >&2
    die "key material under $KEYS is group/world accessible or not owned by root. Fix: chown -R root:root $KEYS && chmod 700 $KEYS && find $KEYS -type f -exec chmod 400 {} +"
  fi
  if find "$KEYS" -mindepth 1 -type l | grep -q .; then
    die "symlinks under $KEYS are not allowed (LoadCredential would follow them)"
  fi
  if [ "$ROLE" = hot ] && find "$KEYS" -maxdepth 1 -name 'armset-signing*' | grep -q .; then
    die "the ArmSet signing key must live only on the analytics box (docs/06 section 12); remove it from $KEYS"
  fi
  if [ "$ROLE" = analytics ] && find "$KEYS" -maxdepth 1 \( -name 'hot-keypair*' -o -name 'probe-keypair*' \) | grep -q .; then
    die "hot/probe wallets must not live on the analytics box (docs/06 section 12); remove them from $KEYS"
  fi
  log "keys: $KEYS ok ($(find "$KEYS" -mindepth 1 -type f | wc -l) file(s), all root-only)"
}

# expand_cpus "0-3,8" -> "0 1 2 3 8"
expand_cpus() {
  local part a b i parts out=()
  IFS=, read -r -a parts <<< "$1"
  for part in "${parts[@]}"; do
    [ -n "$part" ] || continue
    a=${part%-*}; b=${part#*-}
    for ((i = a; i <= b; i++)); do out+=("$i"); done
  done
  echo "${out[@]}"
}

# cpus_minus ALL SUB -> comma list of CPUs in ALL but not in SUB
cpus_minus() {
  local c s keep out=()
  for c in $(expand_cpus "$1"); do
    keep=1
    for s in $(expand_cpus "$2"); do [ "$c" = "$s" ] && keep=0; done
    [ "$keep" = 1 ] && out+=("$c")
  done
  local IFS=,
  echo "${out[*]}"
}

pkg_missing() {
  local p out=()
  for p in "$@"; do
    dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed' || out+=("$p")
  done
  printf '%s\n' "${out[@]}"
}

# ------------------------------------------------------------------------------- preflight
[ "$DRY_RUN" = 1 ] && log "DRY RUN: nothing will be changed"
if [ "$DRY_RUN" = 0 ] && [ "$(id -u)" -ne 0 ]; then
  die "must run as root (sudo), or pass --dry-run"
fi

OS_ID=unknown; OS_VER=unknown
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  OS_ID=${ID:-unknown}; OS_VER=${VERSION_ID:-unknown}
fi
if [ "$OS_ID" != ubuntu ] || { [ "$OS_VER" != 22.04 ] && [ "$OS_VER" != 24.04 ]; }; then
  [ "$FORCE" = 1 ] || die "supported: Ubuntu 22.04/24.04 (found $OS_ID $OS_VER); --force to try anyway"
  warn "unsupported OS $OS_ID $OS_VER, continuing because of --force"
fi
[ -f "$REPO/config/rugsnipe.example.toml" ] || die "run from a rugsnipe checkout ($REPO has no config/rugsnipe.example.toml)"
[ -d "$REPO/deploy/systemd" ] || die "$REPO/deploy/systemd missing"
[ "$REPO" != "$SRC" ] || die "run install.sh from your own clone (e.g. ~/rugsnipe), not from $SRC"
log "role=$ROLE user=$SVC_USER prefix=$PREFIX repo=$REPO os=$OS_ID $OS_VER"

check_keys

ON_EC2=0
if grep -qi amazon /sys/class/dmi/id/sys_vendor /sys/class/dmi/id/board_vendor 2>/dev/null \
   || grep -qi '^ec2' /sys/hypervisor/uuid 2>/dev/null; then
  ON_EC2=1
fi

# ------------------------------------------------------------------------------- 1. packages
PKGS=(ca-certificates curl git rsync jq bc iputils-ping sqlite3 chrony build-essential pkg-config
      libssl-dev protobuf-compiler python3 python3-venv python3-pip wireguard-tools)
[ "$ROLE" = hot ] && PKGS+=(ethtool numactl)

PYBIN=python3
if [ "$OS_VER" = 22.04 ] || { command -v python3 >/dev/null && ! python3 -c 'import sys; sys.exit(sys.version_info < (3, 11))'; }; then
  # Ubuntu 22.04 ships 3.10; analysis/pyproject.toml requires >= 3.11 (python3.11 is in jammy).
  PKGS+=(python3.11 python3.11-venv)
  PYBIN=python3.11
fi

if [ "$DO_APT" = 1 ]; then
  mapfile -t MISSING < <(pkg_missing "${PKGS[@]}" | sed '/^$/d')
  if [ "${#MISSING[@]}" -gt 0 ]; then
    log "apt: installing ${MISSING[*]}"
    run env DEBIAN_FRONTEND=noninteractive apt-get update -q
    run env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends "${MISSING[@]}"
  else
    log "apt: all ${#PKGS[@]} packages present"
  fi
else
  log "apt: skipped (--no-apt)"
fi

# ------------------------------------------------------------------------------- 2. user + dirs
if getent passwd "$SVC_USER" >/dev/null; then
  log "user $SVC_USER exists"
else
  log "creating system user $SVC_USER"
  run useradd --system --user-group --home-dir "$STATE" --no-create-home \
    --shell /usr/sbin/nologin --comment "rugsnipe services" "$SVC_USER"
fi

ensure_dir "$PREFIX" 0755 root:root
ensure_dir "$BIN" 0755 root:root
ensure_dir "$ETC" 0750 "root:$SVC_USER"
ensure_dir "$KEYS" 0700 root:root
ensure_dir "$STATE" 0750 "$SVC_USER:$SVC_USER"
for d in rslog state armsets data; do
  ensure_dir "$STATE/$d" 0750 "$SVC_USER:$SVC_USER"
done

# tmpfiles.d: /run/rugsnipe (sticky, so services cannot delete root's KILL file) + state dirs
sed "s/rugsnipe rugsnipe/$SVC_USER $SVC_USER/; s/root     rugsnipe/root     $SVC_USER/" \
  "$REPO/deploy/systemd/rugsnipe.tmpfiles" | put_file /etc/tmpfiles.d/rugsnipe.conf 0644 root:root
run systemd-tmpfiles --create /etc/tmpfiles.d/rugsnipe.conf

# ------------------------------------------------------------------------------- 3. source tree
# Root-owned copy of the checkout. data/ is NOT copied: $SRC/data is a symlink to
# $STATE/data so the package's caches and reports (analysis/rugsnipe_analysis/cache.py
# resolves data/ relative to the source tree) land in /var/lib/rugsnipe.
RSYNC_EXCLUDES=(--exclude=/.git/ --exclude=/.venv/ --exclude=/target/ --exclude=/data
                --exclude=__pycache__/ --exclude='*.egg-info/' --exclude=/keys/ --exclude=/secrets/
                --exclude='*.keypair.json' --exclude='*-keypair.json' --exclude='*.pem'
                --exclude=/config/rugsnipe.toml)
# The checkout's tracked pre-commit hook refuses key material (files under keys/, keypair
# JSON arrays, PEM private keys; docs/06 section 13). Enable it as the checkout's owner (root
# writing .git/config would leave it root-owned, and git refuses repos owned by someone else).
if [ -d "$REPO/.git" ] && [ -x "$REPO/.githooks/pre-commit" ]; then
  REPO_OWNER=$(stat -c %U "$REPO/.git")
  if [ "$(id -u)" = 0 ] && [ "$REPO_OWNER" != root ]; then
    run runuser -u "$REPO_OWNER" -- git -C "$REPO" config core.hooksPath .githooks
  else
    run git -C "$REPO" config core.hooksPath .githooks
  fi
  log "git: core.hooksPath = .githooks (pre-commit refuses key material)"
fi
log "syncing $REPO -> $SRC"
run rsync -a --delete "${RSYNC_EXCLUDES[@]}" "$REPO/" "$SRC/"
REV=$(git -C "$REPO" rev-parse HEAD 2>/dev/null || echo unknown)
DIRTY=$(git -C "$REPO" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
printf 'commit=%s dirty_files=%s installed=%s role=%s\n' "$REV" "$DIRTY" "$(date -u +%FT%TZ)" "$ROLE" \
  | put_file "$PREFIX/REVISION" 0644 root:root
if [ -d "$REPO/data" ]; then
  # seed committed datasets/reports; never clobber newer files produced on this box
  run rsync -a --update --exclude=/cache/ --exclude=/raw/ "$REPO/data/" "$STATE/data/"
  # chown only what is not ours yet (the cache can hold millions of files)
  run find "$STATE/data" -path "$STATE/data/cache" -prune -o ! -user "$SVC_USER" -exec chown -h "$SVC_USER:$SVC_USER" {} +
fi
if [ -L "$SRC/data" ] || [ ! -e "$SRC/data" ]; then
  run ln -sfn "$STATE/data" "$SRC/data"
else
  warn "$SRC/data exists and is not a symlink; leaving it (caches will not go to $STATE/data)"
fi

# ------------------------------------------------------------------------------- 4. python venv
if [ -x "$VENV/bin/python" ] && ! "$VENV/bin/python" -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2>/dev/null; then
  warn "existing venv $VENV is older than Python 3.11; recreating"
  run rm -rf "$VENV"
fi
if [ ! -x "$VENV/bin/python" ]; then
  log "creating venv $VENV ($PYBIN)"
  run "$PYBIN" -m venv "$VENV"
fi
run "$VENV/bin/python" -I -m pip install -q --upgrade pip
# editable: the package resolves data/ through $SRC/data -> $STATE/data
run "$VENV/bin/python" -I -m pip install -q -e "$SRC/analysis"
run install -m 0755 "$REPO/deploy/bin/rugsnipe-pyrun" "$BIN/rugsnipe-pyrun"
if [ "$DRY_RUN" = 0 ]; then
  STLOG=$(mktemp /tmp/rugsnipe-selftest.XXXXXX)
  if (cd "$STATE" && runuser -u "$SVC_USER" -- "$VENV/bin/python" -I -m rugsnipe_analysis.selftest) >"$STLOG" 2>&1; then
    log "python selftest: $(tail -1 "$STLOG")"
    rm -f "$STLOG"
  else
    warn "python selftest FAILED (see $STLOG); install continues"
  fi
else
  printf '[dry-run] runuser -u %s -- %s/bin/python -I -m rugsnipe_analysis.selftest\n' "$SVC_USER" "$VENV"
fi

# ------------------------------------------------------------------------------- 5. rust
HAVE_WORKSPACE=0
if [ -f "$REPO/Cargo.toml" ] && compgen -G "$REPO/crates/*/Cargo.toml" >/dev/null; then
  HAVE_WORKSPACE=1
fi
TOOLCHAIN=""
if [ -f "$REPO/rust-toolchain.toml" ]; then
  TOOLCHAIN=$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$REPO/rust-toolchain.toml" | head -1)
fi

if [ "$DO_BUILD" = 0 ]; then
  log "rust: skipped (--no-build)"
elif [ "$HAVE_WORKSPACE" = 0 ]; then
  log "rust: no workspace in $REPO; skipped"
else
  [ -n "$TOOLCHAIN" ] || die "rust-toolchain.toml has no channel; refusing to build with an unpinned toolchain"
  export RUSTUP_HOME="$RUSTUP_HOME_DIR" CARGO_HOME="$CARGO_HOME_DIR" CARGO_TARGET_DIR="$TARGET_DIR"
  if [ ! -x "$CARGO_HOME_DIR/bin/rustup" ]; then
    case "$(uname -m)" in
      x86_64) TRIPLE=x86_64-unknown-linux-gnu ;;
      aarch64) TRIPLE=aarch64-unknown-linux-gnu ;;
      *) die "unsupported arch $(uname -m)" ;;
    esac
    URL="https://static.rust-lang.org/rustup/dist/$TRIPLE/rustup-init"
    log "rust: installing rustup ($URL, sha256-verified) toolchain $TOOLCHAIN"
    if [ "$DRY_RUN" = 0 ]; then
      TMPD=$(mktemp -d)
      curl -fsSL --proto '=https' --tlsv1.2 -o "$TMPD/rustup-init" "$URL"
      curl -fsSL --proto '=https' --tlsv1.2 -o "$TMPD/rustup-init.sha256" "$URL.sha256"
      want=$(awk '{print $1}' "$TMPD/rustup-init.sha256")
      have=$(sha256sum "$TMPD/rustup-init" | awk '{print $1}')
      [ "$want" = "$have" ] || die "rustup-init sha256 mismatch ($have != $want)"
      chmod +x "$TMPD/rustup-init"
      "$TMPD/rustup-init" -y --no-modify-path --profile minimal --default-toolchain "$TOOLCHAIN"
      rm -rf "$TMPD"
    else
      printf '[dry-run] download %s + .sha256, verify, rustup-init -y --no-modify-path --profile minimal --default-toolchain %s\n' "$URL" "$TOOLCHAIN"
    fi
  fi
  run "$CARGO_HOME_DIR/bin/rustup" toolchain install "$TOOLCHAIN" --profile minimal -c rustfmt -c clippy
  log "rust: cargo build --release --locked --workspace (toolchain $TOOLCHAIN, target $TARGET_DIR)"
  if [ "$DRY_RUN" = 0 ]; then
    if ! (cd "$SRC" && "$CARGO_HOME_DIR/bin/cargo" "+$TOOLCHAIN" build --release --locked --workspace); then
      # the hot role cannot run without its binaries; the analytics role only loses rugsnipe-ctl
      [ "$ROLE" = hot ] && die "cargo build failed"
      warn "cargo build failed; continuing (the analytics role does not need the Rust binaries)"
    fi
  else
    printf '[dry-run] (cd %s && cargo +%s build --release --locked --workspace)\n' "$SRC" "$TOOLCHAIN"
  fi
  for b in "${RUST_BINS[@]}"; do
    if [ -x "$TARGET_DIR/release/$b" ]; then
      run install -m 0755 "$TARGET_DIR/release/$b" "$BIN/$b"
      log "rust: installed $BIN/$b"
    fi
  done
fi
[ "$ROLE" = hot ] && run install -m 0755 "$REPO/deploy/tune-hot.sh" "$BIN/tune-hot.sh"

# ------------------------------------------------------------------------------- 6. config
if [ -f "$ETC/rugsnipe.toml" ]; then
  log "config: $ETC/rugsnipe.toml exists; not overwritten"
  run chown "root:$SVC_USER" "$ETC/rugsnipe.toml"
  run chmod 0640 "$ETC/rugsnipe.toml"
else
  if [ "$ROLE" = analytics ]; then
    # ohio1 never sends: box id/role, singleton off, paper mode stays.
    sed -e 's/^id = "fra1"/id = "ohio1"/' -e 's/^role = "hot"/role = "analytics"/' \
        -e 's/^singleton = true/singleton = false/' -e 's/"10.77.0.2:9464"/"10.77.0.1:9464"/' \
        "$REPO/config/rugsnipe.example.toml"
  else
    cat "$REPO/config/rugsnipe.example.toml"
  fi | put_file "$ETC/rugsnipe.toml" 0640 "root:$SVC_USER"
  [ "$DRY_RUN" = 1 ] || log "config: created $ETC/rugsnipe.toml from config/rugsnipe.example.toml (mode = paper)"
fi

# ------------------------------------------------------------------------------- 7. chrony
NTP_CONF=/etc/chrony/sources.d/rugsnipe.sources
if [ "$ROLE" = hot ]; then
  # Jito regional NTP: the same time base as the block engines, so cross-box latency logs
  # are comparable (docs/03 section 7). Two neighbours as backup.
  case "$NTP_REGION" in
    frankfurt) NTP_ALT="amsterdam london" ;;
    amsterdam) NTP_ALT="frankfurt london" ;;
    london) NTP_ALT="amsterdam dublin" ;;
    dublin) NTP_ALT="london amsterdam" ;;
    ny) NTP_ALT="slc london" ;;
    slc) NTP_ALT="ny tokyo" ;;
    tokyo) NTP_ALT="singapore slc" ;;
    singapore) NTP_ALT="tokyo frankfurt" ;;
  esac
  {
    echo "# rugsnipe hot role: Jito regional NTP ($NTP_REGION). Managed by deploy/install.sh."
    echo "server ntp.$NTP_REGION.jito.wtf iburst prefer minpoll 4 maxpoll 6"
    for r in $NTP_ALT; do echo "server ntp.$r.jito.wtf iburst minpoll 6 maxpoll 8"; done
  } | put_file "$NTP_CONF" 0644 root:root
elif [ "$ON_EC2" = 1 ]; then
  {
    echo "# rugsnipe analytics role on EC2: Amazon Time Sync. Managed by deploy/install.sh."
    echo "server 169.254.169.123 prefer iburst minpoll 4 maxpoll 4"
  } | put_file "$NTP_CONF" 0644 root:root
else
  log "chrony: analytics role off EC2, keeping distribution defaults"
fi
if [ -f /etc/chrony/chrony.conf ] && ! grep -Eq '^[[:space:]]*sourcedir[[:space:]]+/etc/chrony/sources.d' /etc/chrony/chrony.conf; then
  log "chrony: adding 'sourcedir /etc/chrony/sources.d' to chrony.conf"
  if [ "$DRY_RUN" = 0 ]; then
    printf '\n# added by rugsnipe install.sh\nsourcedir /etc/chrony/sources.d\n' >> /etc/chrony/chrony.conf
  else
    printf '[dry-run] append "sourcedir /etc/chrony/sources.d" to /etc/chrony/chrony.conf\n'
  fi
fi
if command -v systemctl >/dev/null && [ -d /run/systemd/system ]; then
  run systemctl enable --now chrony.service
  run chronyc reload sources || true
else
  log "chrony: systemd not running here; enable chrony.service manually"
fi

# ------------------------------------------------------------------------------- 8. systemd units
render_unit() {  # render_unit NAME -> stdout (service user substituted)
  sed -e "s/^User=rugsnipe\$/User=$SVC_USER/" -e "s/^Group=rugsnipe\$/Group=$SVC_USER/" \
      -e "s#/opt/rugsnipe/#$PREFIX/#g" "$REPO/deploy/systemd/$1"
}
for u in rugsnipe.target "${UNITS[@]}"; do
  render_unit "$u" | put_file "$UNIT_DIR/$u" 0644 root:root
done

if [ "$ROLE" = hot ]; then
  # CPU pinning: rugsnipe-hot on the isolated cores, guard/keeper on the housekeeping cores.
  ISO=$HOT_CPUS
  [ -n "$ISO" ] || ISO=$(cat /sys/devices/system/cpu/isolated 2>/dev/null || true)
  ALL=$(cat /sys/devices/system/cpu/online 2>/dev/null || echo "0-$(( $(nproc) - 1 ))")
  if [ -n "$ISO" ]; then
    HK=$(cpus_minus "$ALL" "$ISO")
    [ -n "$HK" ] || HK=0
    log "cpus: rugsnipe-hot -> $ISO (isolated), guard/keeper -> $HK"
  else
    warn "no isolated CPUs (isolcpus= not set yet; see deploy/tune-hot.sh). rugsnipe-hot gets all CPUs: $ALL"
    ISO=$ALL; HK=$ALL
  fi
  printf '# generated by deploy/install.sh from the isolated CPU set\n[Service]\nCPUAffinity=\nCPUAffinity=%s\n' "$ISO" \
    | put_file "$UNIT_DIR/rugsnipe-hot.service.d/20-cpus.conf" 0644 root:root
  for u in rugsnipe-guard rugsnipe-keeper; do
    printf '# generated by deploy/install.sh (housekeeping CPUs)\n[Service]\nCPUAffinity=\nCPUAffinity=%s\n' "$HK" \
      | put_file "$UNIT_DIR/$u.service.d/20-cpus.conf" 0644 root:root
  done
fi

if command -v systemctl >/dev/null && [ -d /run/systemd/system ]; then
  run systemctl daemon-reload
  if [ "$DO_ENABLE" = 1 ]; then
    for u in "${ENABLE_UNITS[@]}"; do
      case "$u" in
        rugsnipe-hot.service|rugsnipe-guard.service|rugsnipe-keeper.service)
          b=${u%.service}
          if [ ! -x "$BIN/$b" ] && [ "$DRY_RUN" = 0 ]; then warn "not enabling $u: $BIN/$b not built yet"; continue; fi ;;
      esac
      run systemctl enable "$u"
    done
  fi
else
  log "systemd not running here; skipped daemon-reload/enable"
fi

# ------------------------------------------------------------------------------- 9. final checks
check_keys
for k in hot-keypair.json probe-keypair.json api-keys.toml armset-signing.key; do
  case "$ROLE:$k" in
    analytics:hot-keypair.json|analytics:probe-keypair.json|hot:armset-signing.key) continue ;;
  esac
  [ -f "$KEYS/$k" ] || log "credential $KEYS/$k absent -> unit sees 'ABSENT' (paper mode only)"
done

cat <<EOF

[install] done (role=$ROLE$( [ "$DRY_RUN" = 1 ] && echo ', DRY RUN: nothing changed')).
Next steps (docs/07-deploy.md):
EOF
if [ "$ROLE" = analytics ]; then
  cat <<EOF
  sudo systemctl enable --now rugsnipe-collector.service rugsnipe-discover.timer
  journalctl -u rugsnipe-collector -f          # collector logs
  ls $STATE   # analytics.db, data/reports, rslog
EOF
else
  cat <<EOF
  sudo $BIN/tune-hot.sh --check                 # review OS tuning, then: sudo $BIN/tune-hot.sh --apply
  sudo systemctl enable rugsnipe-tune.service   # re-apply runtime tuning at boot
  sudo systemctl enable --now rugsnipe.target   # keeper, guard, hot (mode = paper in rugsnipe.toml)
  sudo touch /run/rugsnipe/KILL                 # kill latch: buys stop, exits continue
EOF
fi
