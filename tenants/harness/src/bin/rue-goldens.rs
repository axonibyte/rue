//! The only golden writer. Refuses unless `RUE_UPDATE_GOLDENS=1` is set:
//! regenerating goldens is a decision, not a fix, and the test suite is
//! read-only. Writes every artifact `rue_tenants::artifacts` declares and
//! reports each as new, changed or unchanged.

use std::env;
use std::fs;
use std::process::ExitCode;

use rue_tenants::golden::repo_root;

fn main() -> ExitCode {
    if env::var("RUE_UPDATE_GOLDENS").ok().as_deref() != Some("1") {
        eprintln!("rue-goldens: refusing to write; set RUE_UPDATE_GOLDENS=1 to regenerate goldens deliberately");
        return ExitCode::from(2);
    }
    let root = match repo_root() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rue-goldens: {e}");
            return ExitCode::from(2);
        }
    };
    let mut written = 0usize;
    let mut failed = 0usize;
    for a in rue_tenants::artifacts() {
        let path = root.join(&a.path);
        match a.bytes {
            Err(e) => {
                eprintln!("rue-goldens: could not produce {}: {e}", a.path);
                failed += 1;
            }
            Ok(bytes) => {
                let status = match fs::read(&path) {
                    Ok(existing) if existing == bytes => "unchanged",
                    Ok(_) => "changed",
                    Err(_) => "new",
                };
                if status != "unchanged" {
                    if let Some(parent) = path.parent() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            eprintln!("rue-goldens: {}: {e}", parent.display());
                            failed += 1;
                            continue;
                        }
                    }
                    if let Err(e) = fs::write(&path, &bytes) {
                        eprintln!("rue-goldens: {}: {e}", path.display());
                        failed += 1;
                        continue;
                    }
                }
                println!("{status:<10} {}", a.path);
                written += 1;
            }
        }
    }
    println!("rue-goldens: {written} artifacts written");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
