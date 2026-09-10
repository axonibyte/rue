#!/bin/sh
# The conformance hook of docs/sdk-conformance.md, written as a command the
# `rue-hook` shim hands each request to on stdin. POSIX sh and sed, no JSON
# library: what this file demonstrates is that a hook needs neither an SDK
# nor a parser, only the ability to read a line and write one.
#
# The shim supplies the `id`, so no reply here carries one.
set -u
read -r req || exit 0

field() { printf '%s' "$req" | sed -n "s/.*\"$1\":\"\\([^\"]*\\)\".*/\\1/p"; }
kind=$(field kind)
op=$(field op)

case "$kind.$op" in
  journal.append)   printf '{"ok":true}\n' ;;
  inventory.list)
    printf '{"ok":true,"hosts":[{"name":"conform-full","address":"198.51.100.7",'
    printf '"os":"freebsd","roles":["a","b"],"reach":["hook"],"filesystem":true,'
    printf '"stdin_preamble":false,"scheduler":"cron","rue_root":"/var/db/rue",'
    printf '"artifact":"python","facts":{"site":"west"}},'
    printf '{"name":"conform-bare","os":"linux"}]}\n' ;;
  execute.run)
    # The secret the request carried. Keys are sorted on the wire, so a
    # resolved value is `"secret":<bool>,"text":"..."`; the secret one is
    # the only `"secret":true`.
    secret=$(printf '%s' "$req" | sed -n 's/.*"secret":true,"text":"\([^"]*\)".*/\1/p')
    cmd=$(printf '%s' "$req" | sed -n 's/.*"cmd":{"secret":false,"text":"\([^"]*\)".*/\1/p')
    printf '{"ok":true,"output":{"stdout":"ran 1 primitive\\n","outputs":'
    printf '{"echo":"%s","secret":"%s"}}}\n' "$cmd" "$secret" ;;
  execute.read_fact)
    case $(field shape) in
      file:/conformance/present) printf '{"ok":true,"content":"present\\n"}\n' ;;
      *) printf '{"ok":true}\n' ;;
    esac ;;
  execute.bootstrap_state)
    printf '{"ok":true,"state":{"rue_root":true,"group":true,"instances_dir":true,'
    printf '"lock":true,"modes_ok":true}}\n' ;;
  execute.clock)         printf '{"ok":true,"epoch_s":1700000000}\n' ;;
  execute.instance_dir_list)
    printf '{"ok":true,"dirs":[{"instance":"conform-1","armed":true,"fired":false,'
    printf '"modes_ok":true}]}\n' ;;
  execute.get_file)      printf '{"ok":true,"content":"1700000000\\n"}\n' ;;
  execute.instance_dir_create|execute.instance_dir_remove|execute.put_file|\
  execute.replace_file|execute.remove_file|execute.host_lock)
    printf '{"ok":true}\n' ;;
  probe.observe)
    case $(field probe) in
      conform-yes)           printf '{"ok":true,"fact":{"text":"yes","tri":"yes"}}\n' ;;
      conform-no)            printf '{"ok":true,"fact":{"text":"no","tri":"no"}}\n' ;;
      conform-unknown)       printf '{"ok":true,"fact":{"text":"","tri":"unknown"}}\n' ;;
      conform-refuse)        printf '{"ok":false,"error":"refused as the contract asks"}\n' ;;
      # The three deliberate violations. The shim passes a reply through as
      # written, which is what makes them expressible at all.
      conform-missing-field) printf '{"ok":true}\n' ;;
      conform-no-ok)         printf '{}\n' ;;
      conform-silent)        exit 0 ;;
      *)                     printf '{"ok":false,"error":"no probe by that name"}\n' ;;
    esac ;;
  approval.authenticators)
    printf '{"ok":true,"authenticators":[{"id":"conform-human","human":true},'
    printf '{"id":"conform-machine","human":false}]}\n' ;;
  approval.challenge)
    printf '{"ok":true,"challenge":"approve %s"}\n' "$(field digest)" ;;
  approval.verify)
    # The proof is bound to the digest and the scope both (5.11), so it is
    # rebuilt from what this request carries and never from anything kept.
    digest=$(field digest)
    case "$req" in
      *'"scope":"plan"'*)  want="$digest/plan" ;;
      *'"scope":{"step":'*) n=$(printf '%s' "$req" | sed -n 's/.*"scope":{"step":\([0-9]*\)}.*/\1/p')
                            want="$digest/step/$n" ;;
      *'"scope":{"ack":'*)  n=$(printf '%s' "$req" | sed -n 's/.*"scope":{"ack":\([0-9]*\)}.*/\1/p')
                            want="$digest/ack/$n" ;;
      *) want="" ;;
    esac
    if [ -n "$want" ] && [ "$(field proof)" = "$want" ]; then
        printf '{"ok":true,"verified":true,"reason":""}\n'
    else
        printf '{"ok":true,"verified":false,"reason":"made for another request or scope"}\n'
    fi ;;
  secrets.resolve)  printf '{"ok":true,"value":"conformance-resolved-secret"}\n' ;;
  secrets.deliver)
    if [ "$(field label)" = unwanted ]; then
        printf '{"ok":true,"accepted":false,"receipt":"receipt-unwanted"}\n'
    else
        printf '{"ok":true,"accepted":true,"receipt":"receipt-conform"}\n'
    fi ;;
  notify.deliver)   printf '{"ok":true}\n' ;;
  scheduler.present)
    case $(field artifact) in
      conform-present.sh) printf '{"ok":true,"present":true}\n' ;;
      conform-absent.sh)  printf '{"ok":true,"present":false}\n' ;;
      # Never guess: the engine reads `false` as "install it again".
      *)                  printf '{"ok":true,"present":"unknown"}\n' ;;
    esac ;;
  scheduler.install|scheduler.arm|scheduler.rearm|scheduler.disarm)
    printf '{"ok":true}\n' ;;
  *)                printf '{"ok":false,"error":"%s.%s is not served here"}\n' "$kind" "$op" ;;
esac
