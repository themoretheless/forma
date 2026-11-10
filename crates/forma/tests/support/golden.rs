//! Golden-file comparison without extra dependencies.
//!
//! `FORMA_BLESS=1 cargo test` rewrites the expected files; a normal run compares against them and
//! reports the first differing line, so a failure points at one behaviour instead of a whole dump.

use std::path::PathBuf;

pub fn assert_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("FORMA_BLESS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!("{}: {error}; record it with FORMA_BLESS=1", path.display())
        })
        // A checkout that converted line endings must not count as a behaviour change.
        .replace("\r\n", "\n");
    if expected == actual {
        return;
    }
    let (mut expected_lines, mut actual_lines) = (expected.lines(), actual.lines());
    for number in 1.. {
        match (expected_lines.next(), actual_lines.next()) {
            (Some(want), Some(got)) if want == got => {}
            (want, got) => panic!(
                "{name} differs at line {number}\n  expected: {}\n  actual:   {}\n\
                 rerun with FORMA_BLESS=1 only if the change is intended",
                want.unwrap_or("<end of file>"),
                got.unwrap_or("<end of file>"),
            ),
        }
    }
}
