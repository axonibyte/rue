#!/bin/sh
# A hook over stdio (docs/hook-protocol.md): registers as the name in $1
# serving execute, then answers every request ok with an empty output. The
# id is read back from the request line; no JSON library, since a stub in
# POSIX sh is the point: any language that can read and write lines can be
# a hook.
name=${1:-act}
printf '{"register":{"name":"%s","kinds":["execute"],"protocol":1}}\n' "$name"
# The acknowledgement.
IFS= read -r _ack || exit 1
while IFS= read -r line; do
    id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
    printf '{"id":%s,"ok":true,"output":{"stdout":"","outputs":{}},"facts":[]}\n' "${id:-0}"
done
