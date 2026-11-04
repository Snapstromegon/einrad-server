#!/usr/bin/env bash
# Checks that the release binaries run on x86-64 CPUs without AVX (#284):
#
#   .github/scripts/check-cpu-baseline.sh <einrad-server> [<other binary>...]
#
# 1. Disassembles every binary: no SQLite function (sqlite3*) may use AVX,
#    AVX2, AVX-512 (VEX/EVEX), BMI or MOVBE instructions. Rust code may, as
#    memchr, ring and co. check the CPU at runtime before they use them.
# 2. Starts the server on an emulated Nehalem CPU (x86-64-v2, no AVX) against
#    a temp database, without config or .env, and waits until it has run the
#    migrations and starts the HTTP server. SMTP is only configured, not used.
#
# The C code is built for x86-64 (v1); this checks the floor that crashed,
# x86-64-v2 CPUs without AVX, not every instruction newer than v1.
#
# Needs objdump and qemu-x86_64 (Debian/Ubuntu: binutils, qemu-user); OBJDUMP
# and QEMU override them, e.g. x86_64-linux-gnu-objdump on another host.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 <einrad-server> [<other binary>...]" >&2
  exit 2
fi

OBJDUMP=${OBJDUMP:-objdump}
QEMU=${QEMU:-qemu-x86_64}
CPU=Nehalem
TIMEOUT=180
failed=0

# Prints "<function> <instruction>" for every instruction newer than
# x86-64-v2 in a sqlite3* function: VEX/EVEX (v*), AVX-512 mask registers
# (k*), BMI and MOVBE. Prefixes like lock or rep come before the mnemonic.
newer_instructions() {
  "$OBJDUMP" -d --no-show-raw-insn "$1" | awk '
    /^[0-9a-f]+ <.*>:$/ { fn = substr($2, 2, length($2) - 3); next }
    fn !~ /^sqlite3/ { next }
    {
      i = 2
      while ($i ~ /^(lock|rep|repz|repnz|repe|repne|notrack|bnd|data16|addr32|[c-gs]s)$/) i++
      if ($i ~ /^v/ || $i ~ /^k(mov|and|or|xor|not|add|shift|test|unpck)/ ||
          $i ~ /^(andn|bextr|blsi|blsmsk|blsr|bzhi|mulx|pdep|pext|rorx|sarx|shlx|shrx|movbe)$/)
        print fn, $i
    }'
}

for binary in "$@"; do
  found=$(newer_instructions "$binary")
  if [ -n "$found" ]; then
    echo "::error::$binary: $(wc -l <<<"$found") AVX/BMI instructions in SQLite, e.g.:"
    # sed reads to the end, so pipefail sees no SIGPIPE.
    sort <<<"$found" | uniq -c | sort -rn | sed -n 1,5p
    failed=1
  elif ! "$OBJDUMP" -t "$binary" | grep ' sqlite3_initialize$' >/dev/null; then
    # No symbols, nothing checked: don't pass silently. (No grep -q: it would
    # stop reading, and pipefail would see objdump's SIGPIPE.)
    echo "::error::$binary: no sqlite3_initialize symbol to check"
    failed=1
  else
    echo "$binary: no AVX/BMI instructions in SQLite"
  fi
done

server=$(realpath "$1")
tmp=$(mktemp -d)
pid=
cleanup() {
  if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; fi
  rm -rf "$tmp"
}
trap cleanup EXIT

# Its own directory, so neither config.toml nor .env apply. No core dumps.
cd "$tmp"
ulimit -c 0
env -i PATH="$PATH" HOME="$tmp" \
  DATABASE_URL="sqlite://$tmp/db.sqlite?mode=rwc" DATA_PATH="$tmp/data" \
  HTTP_ADDRESS=127.0.0.1:0 SMTP_SERVER=smtp.invalid SMTP_USER=ci@example.com \
  SMTP_PASSWORD=unused JWT_SECRET=check-cpu-baseline \
  ADMIN='{"name": "CI Admin", "password": "check-cpu-baseline", "email": "ci@example.com"}' \
  "$QEMU" -cpu "$CPU" "$server" >"$tmp/log" 2>&1 &
pid=$!

status=
for _ in $(seq "$TIMEOUT"); do
  if grep -q 'Starting HTTP server' "$tmp/log"; then
    status=started
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid" && status=0 || status=$?
    pid=
    break
  fi
  sleep 1
done

if [ "$status" = started ]; then
  echo "$server: opened the database and ran the migrations on a $CPU CPU"
else
  cat "$tmp/log"
  if [ -z "$status" ]; then
    echo "::error::$server didn't start the HTTP server on a $CPU CPU within ${TIMEOUT}s"
  elif [ "$status" = 132 ] || grep -q 'signal 4 (Illegal instruction)' "$tmp/log"; then
    echo "::error::$server crashed with SIGILL on a $CPU CPU (no AVX)"
  else
    echo "::error::$server exited with $status on a $CPU CPU before starting the HTTP server"
  fi
  failed=1
fi

exit "$failed"
