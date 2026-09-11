# rue-hook-sdk for Rust

A hook is a process that `rued` calls on a site's behalf: to record journal
entries, run steps on hosts it cannot reach itself, answer probes, approve
gates, resolve and receive secrets, deliver notifications, or schedule
backstops. The protocol is newline-delimited JSON ([hook-protocol.md]).
This crate is the reference SDK for it: a trait per kind, typed in the
records of `rue-hook-proto`, and a serve loop that does the registration
handshake, the framing and the reply shapes for you. Every other SDK in
`sdk/` is written to match it.

- **Typed.** Requests and replies are the protocol's own records
  (`rue_hook_sdk::proto`), not `serde_json::Value`, except where the
  protocol itself carries free-form JSON (a journal entry, an approval
  scope).
- **Conformance-tested.** `rue sdk-conform` drives every op of every kind
  through this crate's serve loop ([sdk-conformance.md]).
- **Protocol v1**, which is frozen: a hook written against it keeps working
  until a new protocol version says otherwise.

## Install

The crate is `rue-hook-sdk`, part of rue's workspace, and is not on
crates.io. Depend on it from a checkout of rue, or through a `git`
dependency on rue's repository at a release tag; the traits take
`serde_json::Value` where the protocol does, so a hook needs `serde_json`
too:

```toml
[dependencies]
rue-hook-sdk = { path = "../rue/sdk/rust" }
serde_json = "1"
```

## Quick start

An audit hook: a journal sink that keeps every entry the engine chains,
and a notifier. This file is `examples/audit_hook.rs`; the crate's own
tests include it and drive it (`tests/audit_example.rs`).

<!-- example: examples/audit_hook.rs -->
```rust
//! An audit hook: a journal sink that keeps every entry, and a notifier.
//!
//! Bind it in a site with `journal to: local(), hook(:audit)` and
//! `notify via: hook(:audit)`, and have rued spawn it:
//!
//!     rued run --spawn audit=/usr/local/libexec/audit_hook ...
//!
//! Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
//! as one line of JSON. A sink that cannot record an entry must say so: the
//! engine then refuses to proceed (R0304) rather than run a step nobody
//! recorded. Notifications go to stderr, because stdout carries the protocol.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use rue_hook_sdk::{serve_stdio, Answer, Hooks, Journal, Notify, Refusal, ServeOptions};
use serde_json::Value;

pub struct AuditLog {
    pub path: PathBuf,
}

impl Journal for AuditLog {
    fn append(&mut self, entry: &Value) -> Answer<()> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(format!("{entry}\n").as_bytes()))
            .map_err(|e| {
                let path = self.path.display();
                Refusal::new(format!("the audit log {path} is not writable: {e}"))
            })
    }
}

pub struct Stderr;

impl Notify for Stderr {
    fn deliver(&mut self, level: &str, subject: &str, body: &str) -> Answer<()> {
        eprintln!("[{level}] {subject}: {body}");
        Ok(())
    }
}

pub fn hooks(path: PathBuf) -> Hooks {
    let mut hooks = Hooks::new();
    hooks.journal = Some(Box::new(AuditLog { path }));
    hooks.notify = Some(Box::new(Stderr));
    hooks
}

// `pub` only so that the SDK's own tests can include this file and name it.
pub fn main() -> std::io::Result<()> {
    let path =
        std::env::var_os("RUE_AUDIT_LOG").map_or_else(|| "audit.ndjson".into(), PathBuf::from);
    serve_stdio(hooks(path), ServeOptions::new("audit"))
}
```

The protocol is plain lines, so you can drive the hook by hand. After its
registration line it waits for the acknowledgement, then answers one
request per line:

```text
$ cargo build -p rue-hook-sdk --example audit_hook
$ printf '%s\n' '{"register":{"ok":true}}' \
    '{"id":1,"kind":"journal","op":"append","entry":{"seq":1}}' \
    '{"id":2,"kind":"probe","op":"observe","host":"h","probe":"p"}' |
  target/debug/examples/audit_hook
{"register":{"filesystem":false,"kinds":["journal","notify"],"name":"audit","protocol":1,"stdin_preamble":false}}
{"id":1,"ok":true}
{"error":"this hook does not serve probe.observe","id":2,"ok":false}
```

## Wire it into a site

The site names the hook where it wants it used, and declares who may
register it:

```text
site do
  journal to: local(), hook(:audit)
  notify via: hook(:audit)
  hooks do
    registrar :spawned, user: :socket_owner, may_register: [:audit]
  end
  ...
end
```

Then `rued` starts it as a child and talks to it over its stdin and
stdout:

```sh
rued run --site site.rue --store /var/db/rue --socket /var/run/rue/rued.sock \
  --spawn audit=/usr/local/libexec/audit_hook
```

Four names have to agree: the one the hook registers with
(`ServeOptions::new("audit")`), the `NAME` of `--spawn NAME=COMMAND`, the
`hook(:audit)` the site binds, and one in a registrar's `may_register`.
`rued` refuses a child that registers under any other name. A child `rued`
spawned is the socket owner, so its registrar says `user: :socket_owner`. A
hook the journal or the inventory depends on must be spawned this way: the
daemon needs it before it starts listening, so it cannot be one that
connects later.

## Next

- [guide.md](guide.md): every kind and its trait, refusing, secrets, the
  budget, and serving over the control socket.
- [testing.md](testing.md): testing a hook, and judging it with
  `rue sdk-conform`.

[hook-protocol.md]: ../../../docs/hook-protocol.md
[sdk-conformance.md]: ../../../docs/sdk-conformance.md
