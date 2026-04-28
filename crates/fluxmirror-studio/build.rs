// Build glue for fluxmirror-studio.
//
// Compiles the studio-web/ Vite project into studio-web/dist/ ahead of
// the main rust build, so include_dir!() can pick the bundle up at
// compile time. Skipped when FLUXMIRROR_SKIP_WEB_BUILD is set or when
// pnpm is not available on PATH (release CI sets the env var, local
// devs running `cargo run -p fluxmirror-studio` get an automatic
// rebuild).

use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=FLUXMIRROR_SKIP_WEB_BUILD");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let studio_web = manifest_dir
        .join("..")
        .join("..")
        .join("studio-web");

    println!(
        "cargo:rerun-if-changed={}",
        studio_web.join("src").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        studio_web.join("index.html").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        studio_web.join("package.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        studio_web.join("vite.config.ts").display()
    );

    if std::env::var_os("FLUXMIRROR_SKIP_WEB_BUILD").is_some() {
        println!("cargo:warning=FLUXMIRROR_SKIP_WEB_BUILD set — using existing studio-web/dist/");
        ensure_dist_exists(&studio_web);
        return;
    }

    if !studio_web.exists() {
        panic!(
            "studio-web/ directory missing at {}; clone the full workspace.",
            studio_web.display()
        );
    }

    let status = Command::new("pnpm")
        .args(["--silent", "build"])
        .current_dir(&studio_web)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("studio-web pnpm build exited {s}"),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            println!(
                "cargo:warning=pnpm not on PATH — skipping frontend rebuild. \
                 Existing studio-web/dist/ will be embedded as-is."
            );
            ensure_dist_exists(&studio_web);
        }
        Err(e) => panic!("failed to invoke pnpm: {e}"),
    }
}

fn ensure_dist_exists(studio_web: &std::path::Path) {
    let dist = studio_web.join("dist").join("index.html");
    if !dist.exists() {
        panic!(
            "studio-web/dist/index.html missing at {}.\n\
             Either run `pnpm install && pnpm build` inside studio-web, \
             or unset FLUXMIRROR_SKIP_WEB_BUILD so build.rs can do it for you.",
            dist.display()
        );
    }
}
