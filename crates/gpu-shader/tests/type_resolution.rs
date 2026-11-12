//! Types and constant expressions the front end cannot resolve are parse errors, not guesses.
use gpu_shader::back::msl;
use gpu_shader::front::wgsl::{eval_u32_const_expr, parse_str};
use gpu_shader::valid::{Capabilities, ValidationFlags, Validator};
use gpu_shader::{ArraySize, TypeInner};
use std::collections::BTreeMap;

fn eval(expr: &str) -> Option<u32> {
    let constants = BTreeMap::from([("N".to_string(), 8), ("HALF".to_string(), 4)]);
    eval_u32_const_expr(expr, &constants)
}

#[test]
fn constant_expressions_follow_precedence_and_associate_to_the_left() {
    for (expr, expected) in [
        ("8 / 2 / 2", 2),
        ("10 - 3 - 2", 5),
        ("17 % 5 % 3", 2),
        ("N - HALF + 1", 5),
        ("2 + 3 * 4", 14),
        ("(2 + 3) * 4", 20),
        ("1 << 4", 16),
        ("1u << 2u << 1u", 8),
        ("256 >> 4", 16),
        ("1 + 1 << 3", 16),
        ("6 & 3 | 8", 10),
        ("6 ^ 3", 5),
        ("~0u", u32::MAX),
        ("-1", u32::MAX),
        ("N * -1 + N", 0),
        ("u32(N) * 2", 16),
        ("0x10 + 0X0f", 31),
        (" ( N ) ", 8),
    ] {
        assert_eq!(eval(expr), Some(expected), "{expr}");
    }
}

#[test]
fn constant_expressions_that_have_no_value_are_rejected() {
    for expr in [
        "", "N /", "N / 0", "N % 0", "1 << 32", "UNKNOWN", "N N", "(N", "N)", "1.5", "a.b",
    ] {
        assert_eq!(eval(expr), None, "{expr}");
    }
}

fn error(source: &str) -> String {
    match parse_str(source) {
        Ok(_) => panic!("accepted:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn an_unknown_type_is_a_parse_error_at_its_position() {
    let message = error("struct Light {\n    color: vec3<f32>,\n    falloff: Fallof,\n}\n");
    assert!(message.contains("unknown type `Fallof`"), "{message}");
    assert!(message.contains("3:14"), "{message}");

    for source in [
        "var<private> state: Missing;",
        "fn run(input: Missing) {}",
        "fn run() -> Missing {}",
        "const LIMIT: Missing = 4;",
        "struct Grid { cells: array<Missing, 4> }",
    ] {
        let message = error(source);
        assert!(
            message.contains("unknown type `Missing`"),
            "{source}: {message}"
        );
    }
}

#[test]
fn an_array_length_without_a_constant_value_is_a_parse_error() {
    for (length, source) in [
        ("COUNT", "struct Grid { cells: array<u32, COUNT> }"),
        ("0", "struct Grid { cells: array<u32, 0> }"),
        (
            "N / 0",
            "const N = 4; struct Grid { cells: array<u32, N / 0> }",
        ),
    ] {
        let message = error(source);
        assert!(
            message.contains(&format!(
                "array length `{length}` is not a positive constant"
            )),
            "{source}: {message}"
        );
    }
}

/// The size in bytes of the struct named `name`.
fn span(source: &str, name: &str) -> u32 {
    let module = parse_str(source).unwrap();
    let found = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no struct `{name}`"));
    match &found.1.inner {
        TypeInner::Struct { span, .. } => *span,
        other => panic!("`{name}` is {other:?}"),
    }
}

#[test]
fn a_struct_may_be_used_before_it_is_declared() {
    let forward =
        "struct Outer { inner: Inner, tail: u32 }\nstruct Inner { a: vec4<f32>, b: vec4<f32> }";
    let backward =
        "struct Inner { a: vec4<f32>, b: vec4<f32> }\nstruct Outer { inner: Inner, tail: u32 }";
    assert_eq!(span(backward, "Outer"), 48);
    assert_eq!(span(forward, "Outer"), 48);

    let module =
        parse_str("const ORIGIN: Point = Point(0u, 0u);\nstruct Point { x: u32, y: u32 }").unwrap();
    let (_, origin) = module.constants.iter().next().unwrap();
    assert!(matches!(
        module.types[origin.ty.unwrap()].inner,
        TypeInner::Struct { span: 8, .. }
    ));
}

#[test]
fn a_struct_that_contains_itself_is_a_parse_error() {
    for source in [
        "struct Node { next: Node }",
        "struct A { b: B }\nstruct B { a: array<A, 2> }",
    ] {
        let message = error(source);
        assert!(message.contains("contains itself"), "{source}: {message}");
    }
}

#[test]
fn array_lengths_use_the_full_constant_evaluator() {
    let module = parse_str("const N = 8; struct Grid { cells: array<u32, N / 2 / 2> }").unwrap();
    let length = module.types.iter().find_map(|(_, ty)| match ty.inner {
        TypeInner::Array {
            size: ArraySize::Constant(length),
            ..
        } => Some(length.get()),
        _ => None,
    });
    assert_eq!(length, Some(2));
}

fn msl(source: &str) -> Result<String, String> {
    let module = parse_str(source).unwrap();
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap();
    msl::write_string(
        &module,
        &info,
        &msl::Options::default(),
        &Default::default(),
    )
    .map(|(source, _)| source)
}

#[test]
fn array_length_of_something_that_is_not_a_bound_runtime_array_is_rejected() {
    let bound = "
@group(0) @binding(0) var<storage, read> values: array<u32>;
@compute @workgroup_size(1)
fn run() { let count = arrayLength(&values); }";
    assert!(msl(bound).unwrap().contains("_wgpu_buffer_sizes["));

    let unbound = "
@group(0) @binding(0) var<storage, read> values: array<u32>;
@compute @workgroup_size(1)
fn run() { let count = arrayLength(&valeus); }";
    let message = msl(unbound).unwrap_err();
    assert!(message.contains("arrayLength(&valeus)"), "{message}");
}
