#!/bin/sh
# A hook over stdio that lists the site's hosts (docs/hook-protocol.md,
# `inventory.list`) and answers everything else with a bare ok. No JSON
# library: a hook in POSIX sh is the point.
name=${1:-world}
printf '{"register":{"name":"%s","kinds":["inventory","execute"],"protocol":1}}\n' "$name"
# The acknowledgement.
IFS= read -r _ack || exit 1
while IFS= read -r line; do
    id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
    case $line in
        *'"kind":"inventory"'*)
            printf '{"id":%s,"ok":true,"hosts":[{"name":"h","address":"10.0.0.1","os":"freebsd","reach":["api"],"filesystem":false}]}\n' "${id:-0}"
            ;;
        *)
            printf '{"id":%s,"ok":true,"output":{"stdout":"","outputs":{}},"facts":[]}\n' "${id:-0}"
            ;;
    esac
done
