use super::parse_str;

#[test]
fn parse_errors_report_exact_line_and_column() {
    let missing_colon =
        "struct Good {\n    x: f32,\n}\n\nstruct Bad {\n    first: u32,\n    missing_type\n}\n";
    let err = parse_str(missing_colon).unwrap_err();
    assert_eq!((err.line, err.column), (7, 5), "{err}");

    let bad_attr = "// header comment\nconst K: u32 = 4u;\n   @unknown_attr(1)\n@compute @workgroup_size(1)\nfn main() {}\n";
    let err = parse_str(bad_attr).unwrap_err();
    assert_eq!((err.line, err.column), (3, 4), "{err}");

    let bad_space = "/* multi\n   line */\n@group(0) @binding(0) var<bogus> data: u32;\n";
    let err = parse_str(bad_space).unwrap_err();
    assert_eq!((err.line, err.column), (3, 27), "{err}");

    let bad_fn = "\n\nfn broken(a: f32, no_colon) {}\n";
    let err = parse_str(bad_fn).unwrap_err();
    assert_eq!((err.line, err.column), (3, 19), "{err}");
}
