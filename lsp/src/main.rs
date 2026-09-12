//! `rue-lsp`: the language server, over stdio.
//!
//! An editor starts this and speaks LSP to it; there are no arguments and
//! no configuration. What it answers with is in `lib.rs`.

fn main() {
    if let Err(e) = rue_lsp::server::run() {
        eprintln!("rue-lsp: {e}");
        std::process::exit(1);
    }
}
