#!/bin/sh
# rue backstop artifact: plan open_mgmt_port on fw-01 (os freebsd), instance golden, language sh. Rendered by rue-render; do not edit.
set -u
ROOT='/var/db/rue'
INST='/var/db/rue/instances/golden'
[ -e "$INST/fired" ] && exit 0
now=$(date +%s)
due=0
if [ -f "$INST/deadline" ]; then d=$(cat "$INST/deadline"); [ "$now" -ge "$d" ] && due=1; fi
[ "$due" -eq 1 ] || exit 0
if [ -z "${RUE_LOCKED:-}" ]; then
  if command -v lockf >/dev/null 2>&1; then RUE_LOCKED=1 exec lockf -k -t 300 "$ROOT/lock" sh "$0"
  elif command -v flock >/dev/null 2>&1; then RUE_LOCKED=1 exec flock -w 300 "$ROOT/lock" sh "$0"
  else RUE_NOLOCK=1; fi
fi
sha() {
  [ -f "$1" ] || { echo missing; return; }
  if command -v sha256 >/dev/null 2>&1; then sha256 -q "$1"
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
recorded() { awk -v p="$2" '$2 == p { print $3 }' "$1"; }
foreign_region() {
  for m in "$ROOT"/instances/*/manifest; do
    [ "$m" = "$INST/manifest" ] && continue
    [ -f "$m" ] || continue
    grep -q -F "region $1 " "$m" && return 0
  done
  return 1
}
strip_region() {
  [ -f "$1" ] || return 1
  b=$(grep -c -F -x "# rue-region $2 begin" "$1"); e=$(grep -c -F -x "# rue-region $2 end" "$1")
  [ "$b" -eq 1 ] && [ "$e" -eq 1 ] || return 1
  awk -v a="$2" '$0 == "# rue-region " a " begin" { skip = 1; next } $0 == "# rue-region " a " end" { skip = 0; next } !skip { print }' "$1" > "$1.rue-tmp" && mv "$1.rue-tmp" "$1"
}
region_set() {
  strip_region "$1" "$2" || true
  { [ -f "$1" ] && cat "$1"; printf '# rue-region %s begin\n%s\n# rue-region %s end\n' "$2" "$3" "$2"; } > "$1.rue-tmp" && mv "$1.rue-tmp" "$1"
}
restore() { cp "$1" "$2.rue-tmp" && mv "$2.rue-tmp" "$2"; }
defer() { echo "$1" >> "$INST/drift"; }
clobbered() { echo "$1" >> "$INST/clobbered"; }
# step 1: pf_allow
M="$INST/markers/1"
if [ -f "$M" ]; then
  skip=0
  if [ "$skip" -eq 1 ]; then defer 1; else
    strip_region '/etc/pf.conf' 'rue-mgmt' || { if [ -n "${RUE_NOLOCK:-}" ] || foreign_region '/etc/pf.conf'; then defer 1; else restore '/var/db/rue/instances/golden/snapshots/1/0' '/etc/pf.conf'; clobbered 1; fi; }
    rm -f "$M"
  fi
fi
: > "$INST/fired"
exit 0
