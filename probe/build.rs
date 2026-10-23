//! Records the git commit the binary is built from (with `-dirty` for uncommitted changes), so every run.json names
//! the code that produced it. Outside a git checkout (the Docker build) it is `unknown`.

use std::process::Command;

fn main() {
    let git = |args: &[&str]| {
        Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let commit = match git(&["rev-parse", "--short=12", "HEAD"]) {
        Some(hash) => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
            if dirty { format!("{hash}-dirty") } else { hash }
        }
        None => "unknown".to_owned(),
    };
    println!("cargo:rustc-env=ORDER_PROBE_COMMIT={commit}");
    for path in ["../.git/HEAD", "../.git/index", "../.git/refs/heads"] {
        println!("cargo:rerun-if-changed={path}");
    }
}
