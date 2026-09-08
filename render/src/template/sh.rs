//! The POSIX `sh` template: FreeBSD, Linux, macOS. Hashing falls back
//! through `sha256 -q`, `sha256sum` and `shasum -a 256`, the first that
//! exists on those three.

use rue_core::model::Drift;

use super::banner;
use crate::actions::{Action, Kind};
use crate::quote::posix;
use crate::{Context, RenderError};

fn q(step: u32, s: &str) -> Result<String, RenderError> {
    posix(s).map_err(|inner| RenderError::Unquotable { step, inner })
}

pub fn render(ctx: &Context<'_>) -> Result<String, RenderError> {
    let inst = format!("{}/instances/{}", ctx.root, ctx.instance.id);
    let mut o = String::new();
    o.push_str("#!/bin/sh\n");
    o.push_str(&format!("# {}\n", banner(ctx)));
    o.push_str("set -u\n");
    o.push_str(&format!("ROOT={}\n", q(0, &ctx.root)?));
    o.push_str(&format!("INST={}\n", q(0, &inst)?));
    o.push_str("[ -e \"$INST/fired\" ] && exit 0\n");
    o.push_str("now=$(date +%s)\ndue=0\n");
    if ctx.deadline {
        o.push_str(
            "if [ -f \"$INST/deadline\" ]; then d=$(cat \"$INST/deadline\"); [ \"$now\" -ge \"$d\" ] && due=1; fi\n",
        );
    }
    if let Some(hb) = ctx.heartbeat_s {
        o.push_str(&format!(
            "if [ -f \"$INST/heartbeat\" ]; then h=$(cat \"$INST/heartbeat\"); [ $((now - h)) -gt {hb} ] && due=1; else due=1; fi\n"
        ));
    }
    o.push_str("[ \"$due\" -eq 1 ] || exit 0\n");
    // The host lock for the whole run (7.7), so the artifact and the engine
    // never edit a region's file at once: lockf on FreeBSD, flock on Linux.
    // With neither in base (macOS) the run proceeds unlocked and a damaged
    // region is deferred, never restored whole.
    o.push_str(
        "if [ -z \"${RUE_LOCKED:-}\" ]; then\n  if command -v lockf >/dev/null 2>&1; then RUE_LOCKED=1 exec lockf -k -t 300 \"$ROOT/lock\" sh \"$0\"\n  elif command -v flock >/dev/null 2>&1; then RUE_LOCKED=1 exec flock -w 300 \"$ROOT/lock\" sh \"$0\"\n  else RUE_NOLOCK=1; fi\nfi\n",
    );
    o.push_str(HELPERS);
    for s in &ctx.steps {
        let n = s.n;
        o.push_str(&format!("# step {}: {}\n", n, s.id));
        o.push_str(&format!("M=\"$INST/markers/{n}\"\n"));
        o.push_str("if [ -f \"$M\" ]; then\n  skip=0\n");
        for f in &s.files {
            // Under :defer a changed fact is left alone and marked; under
            // :clobber an Owned or Modified change is reported as clobbered
            // (Region damage is decided by strip_region).
            if f.kind == Kind::Region {
                continue;
            }
            let p = q(n, &f.path)?;
            match s.drift {
                Drift::Defer => o.push_str(&format!(
                    "  [ \"$(sha {p})\" = \"$(recorded \"$M\" {p})\" ] || skip=1\n"
                )),
                Drift::Clobber => o.push_str(&format!(
                    "  [ \"$(sha {p})\" = \"$(recorded \"$M\" {p})\" ] || clobbered {n}\n"
                )),
            }
        }
        o.push_str(&format!("  if [ \"$skip\" -eq 1 ]; then defer {n}; else\n"));
        for a in &s.actions {
            o.push_str("    ");
            match a {
                Action::Remove { path } => o.push_str(&format!("rm -f {}\n", q(n, path)?)),
                Action::StripRegion { path, anchor, k } => {
                    let p = q(n, path)?;
                    let snap = q(n, &format!("{inst}/snapshots/{n}/{k}"))?;
                    let fallback = match s.drift {
                        Drift::Clobber => format!(
                            "if [ -n \"${{RUE_NOLOCK:-}}\" ] || foreign_region {p}; then defer {n}; else restore {snap} {p}; clobbered {n}; fi"
                        ),
                        Drift::Defer => format!("defer {n}"),
                    };
                    o.push_str(&format!(
                        "strip_region {p} {} || {{ {fallback}; }}\n",
                        q(n, anchor)?
                    ));
                }
                Action::RestoreSnapshot { path, k } => o.push_str(&format!(
                    "restore {} {}\n",
                    q(n, &format!("{inst}/snapshots/{n}/{k}"))?,
                    q(n, path)?
                )),
                Action::Run { command } => o.push_str(&format!("sh -c {}\n", q(n, command)?)),
                Action::Write { path, content } => o.push_str(&format!(
                    "printf '%s' {} > {}\n",
                    q(n, content)?,
                    q(n, path)?
                )),
                Action::Append { path, line } => o.push_str(&format!(
                    "printf '%s\\n' {} >> {}\n",
                    q(n, line)?,
                    q(n, path)?
                )),
                Action::RegionSet {
                    path,
                    anchor,
                    content,
                } => o.push_str(&format!(
                    "region_set {} {} {}\n",
                    q(n, path)?,
                    q(n, anchor)?,
                    q(n, content)?
                )),
                Action::RegionClear { path, anchor } => o.push_str(&format!(
                    "strip_region {} {} || true\n",
                    q(n, path)?,
                    q(n, anchor)?
                )),
            }
        }
        o.push_str("    rm -f \"$M\"\n  fi\nfi\n");
    }
    o.push_str(": > \"$INST/fired\"\nexit 0\n");
    Ok(o)
}

pub(crate) const HELPERS: &str = r##"sha() {
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
"##;
