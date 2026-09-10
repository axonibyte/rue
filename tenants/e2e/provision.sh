#!/bin/sh
# Provision a disposable reaper guest for rue's tier 5 and 6 tests, or check
# that it is provisioned (docs/TESTING.md, "Under reaper").
#
#   sh tenants/e2e/provision.sh apply     make the guest ready (root)
#   sh tenants/e2e/provision.sh check     assert it is; exit 1 otherwise
#
# What "ready" is, on FreeBSD and on Linux:
#
#   - $E2E_ROOT (rue-e2e beside the working tree; never inside it, never
#     ~/.ssh) holds the harness's Ed25519 key and its own known_hosts naming
#     the guest's host key for the loopback alias.
#   - sshd accepts that key through a drop-in that adds a second
#     AuthorizedKeysFile under $E2E_ROOT; nothing under ~/.ssh is read or
#     written by this script.
#   - The target address 127.0.0.2 answers: a loopback alias on FreeBSD (the
#     whole of 127/8 is on lo already on Linux), so a plan that severs ssh
#     severs only itself, never reaper's transport on the management
#     interface.
#   - The firewall is up with a baseline that skips the management interface
#     (pf: `set skip on <mgmt>`; nftables: a table `inet rue` whose input
#     chain accepts), so a later plan's rules can only ever touch loopback.
#   - The `rue` group exists and rue_root is $REAPER_STATE/rue with the modes
#     of ROADMAP.md section 7.7, under the dataset reaper's reset rolls back.
#
# Refuses to apply anywhere that is not a reaper guest (REAPER_WORK set) unless
# RUE_E2E_DISPOSABLE=1 says the machine may be rewritten. POSIX sh: FreeBSD's
# sh and dash both run it.
set -u

mode=${1:-}
case $mode in
    apply|check) ;;
    *) echo "usage: provision.sh apply|check" >&2; exit 2 ;;
esac

os=$(uname -s)
case $os in
    FreeBSD|Linux) ;;
    *) echo "provision: $os is not a guest this harness provisions" >&2; exit 2 ;;
esac

if [ -n "${RUE_E2E_ROOT:-}" ]; then
    root=$RUE_E2E_ROOT
elif [ -n "${REAPER_WORK:-}" ]; then
    root=$(dirname -- "$REAPER_WORK")/rue-e2e
else
    echo "provision: neither RUE_E2E_ROOT nor REAPER_WORK is set" >&2
    exit 2
fi
state=${REAPER_STATE:-}
rue_root=${RUE_E2E_RUE_ROOT:-${state:+$state/rue}}
if [ -z "$rue_root" ]; then
    echo "provision: neither RUE_E2E_RUE_ROOT nor REAPER_STATE is set; rue_root has nowhere to live" >&2
    exit 2
fi
alias_addr=127.0.0.2
dropin=/etc/ssh/sshd_config.d/rue-e2e.conf

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

mgmt_if() {
    case $os in
        FreeBSD) route -n get default 2> /dev/null | awk '/interface:/ { print $2 }' ;;
        Linux) ip -o route show default 2> /dev/null | awk '{ for (i = 1; i <= NF; i++) if ($i == "dev") print $(i + 1) }' | head -n 1 ;;
    esac
}

sshd_reload() {
    case $os in
        FreeBSD) service sshd reload ;;
        Linux) systemctl reload ssh ;;
    esac
}

# --- apply -----------------------------------------------------------------

apply() {
    if [ -z "${REAPER_WORK:-}" ] && [ "${RUE_E2E_DISPOSABLE:-}" != 1 ]; then
        echo "provision: this is not a reaper guest (REAPER_WORK unset); set RUE_E2E_DISPOSABLE=1 only on a machine that may be rewritten" >&2
        exit 2
    fi
    if [ "$(id -u)" -ne 0 ]; then
        echo "provision: apply needs root" >&2
        exit 2
    fi
    mgmt=$(mgmt_if)
    if [ -z "$mgmt" ]; then
        echo "provision: no default route; the management interface is unknown and a firewall baseline would be a guess" >&2
        exit 2
    fi

    # Key material, beside the tree.
    mkdir -p "$root/keys" || exit 2
    chmod 0700 "$root/keys"
    if [ ! -f "$root/keys/id_ed25519" ]; then
        ssh-keygen -q -t ed25519 -N '' -C 'rue e2e (guest)' -f "$root/keys/id_ed25519" || exit 2
    fi
    # The authorized keys sshd reads for the harness: a file of our own,
    # root-owned and not group-writable, as StrictModes requires.
    cp "$root/keys/id_ed25519.pub" "$root/authorized_keys" || exit 2
    chmod 0644 "$root/authorized_keys"
    chown root "$root" "$root/authorized_keys"
    chmod go-w "$root"
    # Our known_hosts: the guest's own host key, for the alias.
    printf '%s %s\n' "$alias_addr" "$(cut -d' ' -f1,2 /etc/ssh/ssh_host_ed25519_key.pub)" > "$root/known_hosts" || exit 2

    # sshd: a drop-in naming the harness's file beside the default. Debian's
    # sshd_config includes sshd_config.d; FreeBSD's base config does not and
    # sets AuthorizedKeysFile itself, and sshd keeps the first value it
    # reads, so the Include goes at the top of the file (the same Include a
    # FreeBSD target needs for T1's own sshd drop-in).
    if ! grep -q '^Include /etc/ssh/sshd_config.d/\*\.conf' /etc/ssh/sshd_config; then
        { printf 'Include /etc/ssh/sshd_config.d/*.conf\n'; cat /etc/ssh/sshd_config; } > /etc/ssh/sshd_config.tmp || exit 2
        mv /etc/ssh/sshd_config.tmp /etc/ssh/sshd_config
    fi
    mkdir -p /etc/ssh/sshd_config.d
    printf '# rue e2e: the harness key, beside the default file; never ~/.ssh of the harness user.\nAuthorizedKeysFile .ssh/authorized_keys %s\n' "$root/authorized_keys" > "$dropin.tmp" || exit 2
    mv "$dropin.tmp" "$dropin"
    sshd -t || { echo "provision: sshd refuses its configuration with the drop-in" >&2; exit 2; }
    sshd_reload || exit 2

    # The alias.
    if [ "$os" = FreeBSD ]; then
        ifconfig lo0 | grep -q "inet $alias_addr " || ifconfig lo0 alias "$alias_addr/32" || exit 2
    fi

    # The firewall baseline.
    case $os in
        FreeBSD)
            kldstat -q -m pf || kldload pf || exit 2
            printf '# rue e2e baseline: the management interface is never filtered.\nset skip on %s\npass all\n' "$mgmt" > /etc/pf.conf.tmp || exit 2
            mv /etc/pf.conf.tmp /etc/pf.conf
            sysrc -q pf_enable=YES > /dev/null || exit 2
            pfctl -q -f /etc/pf.conf || exit 2
            pfctl -s info | grep -q 'Status: Enabled' || pfctl -q -e || exit 2
            ;;
        Linux)
            printf '# rue e2e baseline: a table of our own whose input chain accepts; a plan may add rules under loopback only.\ntable inet rue {\n\tchain input {\n\t\ttype filter hook input priority 0; policy accept;\n\t}\n}\n' > /etc/nftables.conf.tmp || exit 2
            mv /etc/nftables.conf.tmp /etc/nftables.conf
            nft -f /etc/nftables.conf || exit 2
            ;;
    esac

    # The scheduler baseline: no backstop of an earlier run is still armed.
    # reaper rolls back the state dataset between runs but not /var/cron, so
    # a crontab entry outlives by days the instance directory it names: cron
    # fires it into a missing file every minute, and a test asking whether a
    # backstop is present can be answered by the last run's entry rather than
    # its own -- instance ids are deterministic, so the ids even match. This
    # asserts nothing and weakens nothing; it is the same known starting
    # point the firewall and sshd baselines above establish.
    strip_crontab_regions || exit 2

    # The group and rue_root (section 7.7).
    case $os in
        FreeBSD) pw groupshow rue > /dev/null 2>&1 || pw groupadd rue || exit 2 ;;
        Linux) getent group rue > /dev/null 2>&1 || groupadd rue || exit 2 ;;
    esac
    mkdir -p "$rue_root/instances" || exit 2
    chown root:rue "$rue_root" "$rue_root/instances"
    chmod 0755 "$rue_root"
    chmod 2770 "$rue_root/instances"
    : > "$rue_root/lock"
    chown root:rue "$rue_root/lock"
    chmod 0664 "$rue_root/lock"
    echo "provisioned: mgmt=$mgmt alias=$alias_addr root=$root rue_root=$rue_root"
}

# Remove every `# rue-region <id> begin`..`end` block from the scheduler's
# crontab, leaving anything else in it alone. A crontab with no such block
# is left untouched, so this never rewrites a file it has nothing to say
# about.
strip_crontab_regions() {
    crontab -l 2> /dev/null | grep -q '^# rue-region ' || return 0
    ct=$root/crontab.rue-e2e
    crontab -l 2> /dev/null | awk '
        /^# rue-region .* begin$/ { skip = 1; next }
        /^# rue-region .* end$/   { skip = 0; next }
        !skip
    ' > "$ct" || return 1
    crontab "$ct" || return 1
    rm -f "$ct"
    # Assert the strip here and not in `check`: `check` runs a second time
    # as a test of its own (tenants/e2e/tests/smoke.rs), by which point this
    # run's backstops are armed and an empty crontab would be the bug.
    if crontab -l 2> /dev/null | grep -q '^# rue-region '; then
        echo "provision: the crontab still holds a rue region after stripping" >&2
        return 1
    fi
}

# --- check -----------------------------------------------------------------

# Each check is a function so a pipeline can be judged as one command.
chk() { # chk <label> <command...>
    label=$1; shift
    if "$@" > /dev/null 2>&1; then ok "$label"; else bad "$label"; fi
}
sshd_reads_harness_file() { sshd -T 2> /dev/null | grep -i '^authorizedkeysfile' | grep -q -F "$root/authorized_keys"; }
alias_present() { ifconfig lo0 | grep -q "inet $alias_addr "; }
pf_enabled() { pfctl -s info 2> /dev/null | grep -q 'Status: Enabled'; }
# Only the verbose listing marks skipped interfaces.
pf_skips_mgmt() { pfctl -s Interfaces -v 2> /dev/null | grep -q "^$mgmt (skip)"; }
nft_table_present() { nft list table inet rue; }
nft_input_accepts() { nft list chain inet rue input 2> /dev/null | grep -q 'policy accept'; }
group_rue_exists() {
    case $os in
        FreeBSD) pw groupshow rue ;;
        Linux) getent group rue ;;
    esac
}

check() {
    mgmt=$(mgmt_if)
    chk "management interface is known ($mgmt)" test -n "$mgmt"
    chk "harness key present at $root/keys" test -f "$root/keys/id_ed25519"
    chk "harness known_hosts present" test -s "$root/known_hosts"
    chk "sshd drop-in present at $dropin" test -f "$dropin"
    chk "sshd reads $root/authorized_keys" sshd_reads_harness_file
    case $os in
        FreeBSD)
            chk "loopback alias $alias_addr" alias_present
            chk "pf enabled" pf_enabled
            chk "pf skips $mgmt (a plan's rule can never sever reaper's transport)" pf_skips_mgmt
            ;;
        Linux)
            chk "nftables table inet rue present" nft_table_present
            chk "nftables input chain accepts by policy" nft_input_accepts
            ;;
    esac
    chk "group rue exists" group_rue_exists
    chk "rue_root $rue_root/instances present" test -d "$rue_root/instances"
    exit "$rc"
}

$mode
