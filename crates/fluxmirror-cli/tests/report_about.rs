// Integration test for `fluxmirror about`.
//
// No DB access — just confirms the binary exits 0 and prints all 7
// `/fluxmirror:*` slash command names + a localized title under both
// English and Korean.

use std::process::Command;

fn fluxmirror_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fluxmirror"))
}

const COMMANDS: &[&str] = &[
    "/fluxmirror:today",
    "/fluxmirror:yesterday",
    "/fluxmirror:week",
    "/fluxmirror:compare",
    "/fluxmirror:agents",
    "/fluxmirror:agent",
    "/fluxmirror:about",
];

#[test]
fn about_english_lists_seven_command_names() {
    let output = fluxmirror_bin()
        .args(["about", "--lang", "english"])
        .output()
        .expect("spawn fluxmirror about");
    assert!(
        output.status.success(),
        "non-zero exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    for name in COMMANDS {
        assert!(
            stdout.contains(name),
            "missing command {name} in:\n{stdout}"
        );
    }
    assert!(stdout.contains("About FluxMirror"));
}

#[test]
fn about_korean_translates_title_but_keeps_command_names() {
    let output = fluxmirror_bin()
        .args(["about", "--lang", "korean"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    for name in COMMANDS {
        assert!(
            stdout.contains(name),
            "ko output missing {name}:\n{stdout}"
        );
    }
    assert!(stdout.contains("FluxMirror 소개"));
}

#[test]
fn about_format_json_is_reserved_but_unimplemented() {
    let output = fluxmirror_bin()
        .args(["about", "--lang", "english", "--format", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}
