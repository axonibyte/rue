#!/bin/sh
# An audit hook behind the rue-hook shim: a journal sink that keeps every
# entry, and a notifier, in POSIX sh with no JSON parser.
#
#   rued run --spawn audit="rue-hook --name audit --kinds journal,notify \
#                             --command /usr/local/libexec/audit.sh" ...
#
# The shim hands each request to this script as one line of JSON on its
# stdin, and puts the request's id on the reply itself. A journal request
# is appended to $RUE_AUDIT_LOG (default audit.ndjson) whole, since it
# carries the entry. A sink that cannot record an entry must say so: the
# engine then refuses to proceed (R0304) rather than run a step nobody
# recorded. Notifications go to stderr, because stdout carries the reply.
set -u
read -r req || exit 0

case $req in
  *'"kind":"journal"'*)
    if { printf '%s\n' "$req" >> "${RUE_AUDIT_LOG:-audit.ndjson}"; } 2> /dev/null; then
      printf '{"ok":true}\n'
    else
      printf '{"ok":false,"error":"the audit log is not writable"}\n'
    fi ;;
  *'"kind":"notify"'*)
    printf '%s\n' "$req" >&2
    printf '{"ok":true}\n' ;;
  *)
    printf '{"ok":false,"error":"this hook serves journal and notify only"}\n' ;;
esac
