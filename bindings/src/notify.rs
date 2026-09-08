//! `stdout()`, the notify binding of docs/ROADMAP.md 7.3: one line per
//! message on the daemon's own stdout, which under rc.d or systemd is the
//! service log. It is the built-in floor; anything a shop actually wants
//! (mail, a chat room, a pager) is a hook.

use std::io::Write;

use rue_engine::executor::ExecError;
use rue_engine::notify::{Level, Notify};

#[derive(Debug, Default)]
pub struct Stdout;

impl Notify for Stdout {
    fn name(&self) -> &str {
        "stdout"
    }

    fn deliver(&mut self, level: Level, subject: &str, body: &str) -> Result<(), ExecError> {
        let mut out = std::io::stdout().lock();
        writeln!(out, "rue {}: {subject}: {body}", level.word())
            .and_then(|()| out.flush())
            .map_err(|e| ExecError::Io(e.to_string()))
    }
}
