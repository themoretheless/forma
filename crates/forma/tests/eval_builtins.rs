//! Pins what every expression builtin returns today, including its error texts.
//!
//! The dispatcher in `form_document/eval.rs` had no direct tests. Each builtin is called with every
//! combination of a small pool of typical and awkward arguments, plus hand-picked cases for the
//! ones whose interesting behaviour needs specific shapes. Error messages are part of the record:
//! callers show them and one caller branches on their text.

#[path = "support/golden.rs"]
mod golden;

use forma::form_document::{Expression, Properties, Value};
use std::fmt::Write;

const BUILTINS: &[&str] = &[
    "abs",
    "floor",
    "ceil",
    "round",
    "sqrt",
    "sign",
    "pow",
    "lerp",
    "remap",
    "avg",
    "sum",
    "min",
    "max",
    "len",
    "count",
    "isEmpty",
    "String",
    "Number",
    "Bool",
    "clamp",
    "trim",
    "upper",
    "lower",
    "capitalize",
    "padStart",
    "padEnd",
    "truncate",
    "repeat",
    "contains",
    "includes",
    "indexOf",
    "startsWith",
    "endsWith",
    "replace",
    "slice",
    "split",
    "join",
    "reverse",
    "take",
    "skip",
    "range",
    "formatNumber",
    "unique",
    "find",
    "compact",
    "flatten",
    "chunk",
    "zip",
    "first",
    "last",
    "groupBy",
    "pluck",
    "sortBy",
    "any",
    "all",
    "keys",
    "values",
    "entries",
    "fromEntries",
    "merge",
    "pick",
    "omit",
    "plural",
    "isValid",
    "alpha",
    "lighten",
    "darken",
    "saturate",
    "desaturate",
    "invert",
    "contrast",
    "mix",
    "noSuchFunction",
];

fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn list(items: &[Value]) -> Value {
    Value::List(items.to_vec())
}
fn object(entries: &[(&str, Value)]) -> Value {
    let mut properties = Properties::new();
    for (key, value) in entries {
        properties.insert((*key).into(), value.clone());
    }
    Value::Object(properties)
}

/// Arguments tried in every position: one of each value kind plus the awkward members of each.
fn pool() -> Vec<Value> {
    vec![
        Value::Number(2.5),
        Value::Number(-3.),
        Value::Number(0.),
        text("Hello World"),
        text(""),
        text("#ff8800"),
        list(&[
            Value::Number(3.),
            Value::Number(1.),
            Value::Number(2.),
            Value::Number(1.),
        ]),
        list(&[]),
        object(&[("a", Value::Number(1.)), ("b", text("x"))]),
        Value::Bool(true),
        Value::Null,
    ]
}

/// Shapes the pool cannot reach: records, nested lists, colours, validation rules, wide arities.
fn curated() -> Vec<(&'static str, Vec<Value>)> {
    let records = list(&[
        object(&[("name", text("b")), ("age", Value::Number(2.))]),
        object(&[("name", text("a")), ("age", Value::Number(1.))]),
        object(&[("name", text("b")), ("age", Value::Number(3.))]),
    ]);
    let pairs = list(&[
        list(&[text("k"), Value::Number(1.)]),
        list(&[text("j"), Value::Number(2.)]),
    ]);
    let n = Value::Number;
    vec![
        ("remap", vec![n(5.), n(0.), n(10.), n(0.), n(100.)]),
        ("remap", vec![n(5.), n(1.), n(1.), n(0.), n(100.)]),
        ("lerp", vec![n(0.), n(10.), n(0.25)]),
        ("clamp", vec![n(15.), n(0.), n(10.)]),
        ("clamp", vec![n(5.), n(10.), n(0.)]),
        ("pow", vec![n(10.), n(400.)]),
        ("sqrt", vec![n(-1.)]),
        ("min", vec![n(3.), n(1.), n(2.)]),
        ("max", vec![n(3.), n(1.), n(2.)]),
        ("round", vec![n(-2.5)]),
        ("round", vec![n(2.4999)]),
        ("padStart", vec![text("7"), n(3.), text("0")]),
        ("padEnd", vec![text("7"), n(3.), text("ab")]),
        ("truncate", vec![text("Hello World"), n(5.), text("…")]),
        ("truncate", vec![text("Привет, мир"), n(6.)]),
        ("slice", vec![text("Привет"), n(1.), n(3.)]),
        ("slice", vec![text("Hello"), n(-3.)]),
        ("slice", vec![list(&[n(1.), n(2.), n(3.)]), n(1.), n(9.)]),
        ("indexOf", vec![text("Привет мир"), text("мир")]),
        ("replace", vec![text("a-b-a"), text("a"), text("z")]),
        ("split", vec![text("a,b,,c"), text(",")]),
        (
            "join",
            vec![list(&[text("a"), n(1.), Value::Bool(true)]), text("-")],
        ),
        ("capitalize", vec![text("élan vital")]),
        ("upper", vec![text("straße")]),
        ("repeat", vec![text("ab"), n(3.)]),
        ("repeat", vec![text("ab"), n(100000.)]),
        ("range", vec![n(1.), n(5.)]),
        ("range", vec![n(5.), n(1.)]),
        ("range", vec![n(0.), n(100000.)]),
        ("formatNumber", vec![n(1234567.891), n(2.)]),
        ("formatNumber", vec![n(-0.5)]),
        ("Number", vec![text("12.5")]),
        ("Number", vec![text("abc")]),
        ("Bool", vec![text("false")]),
        ("String", vec![n(1.0)]),
        ("unique", vec![records.clone(), text("name")]),
        ("find", vec![records.clone(), text("name"), text("a")]),
        ("find", vec![records.clone(), text("name"), text("zz")]),
        ("groupBy", vec![records.clone(), text("name")]),
        ("pluck", vec![records.clone(), text("age")]),
        ("sortBy", vec![records.clone(), text("age")]),
        ("sortBy", vec![list(&[text("b"), text("a"), text("c")])]),
        (
            "compact",
            vec![list(&[
                n(0.),
                Value::Null,
                text(""),
                n(1.),
                Value::Bool(false),
            ])],
        ),
        (
            "flatten",
            vec![list(&[list(&[n(1.), list(&[n(2.)])]), n(3.)])],
        ),
        (
            "chunk",
            vec![list(&[n(1.), n(2.), n(3.), n(4.), n(5.)]), n(2.)],
        ),
        ("chunk", vec![list(&[n(1.)]), n(0.)]),
        ("zip", vec![list(&[n(1.), n(2.)]), list(&[text("a")])]),
        ("any", vec![list(&[Value::Bool(false), Value::Bool(true)])]),
        ("all", vec![list(&[Value::Bool(true), n(1.)])]),
        ("fromEntries", vec![pairs.clone()]),
        ("entries", vec![object(&[("z", n(1.)), ("a", n(2.))])]),
        (
            "merge",
            vec![
                object(&[("a", n(1.))]),
                object(&[("a", n(2.)), ("b", n(3.))]),
            ],
        ),
        (
            "pick",
            vec![
                object(&[("a", n(1.)), ("b", n(2.))]),
                list(&[text("b"), text("zz")]),
            ],
        ),
        (
            "omit",
            vec![object(&[("a", n(1.)), ("b", n(2.))]), list(&[text("b")])],
        ),
        (
            "plural",
            vec![n(1.), text("файл"), text("файла"), text("файлов")],
        ),
        (
            "plural",
            vec![n(3.), text("файл"), text("файла"), text("файлов")],
        ),
        (
            "plural",
            vec![n(11.), text("файл"), text("файла"), text("файлов")],
        ),
        ("plural", vec![n(1.), text("file"), text("files")]),
        ("plural", vec![n(2.), text("file"), text("files")]),
        ("isValid", vec![text("user@example.com"), text("email")]),
        ("isValid", vec![text("nope"), text("email")]),
        ("isValid", vec![text("abc"), text("minLength"), n(5.)]),
        ("isValid", vec![text("abc"), text("unknownRule")]),
        ("alpha", vec![text("#ff8800"), n(0.5)]),
        ("alpha", vec![text("#f80"), n(2.)]),
        ("lighten", vec![text("#804020"), n(0.2)]),
        ("darken", vec![text("#804020ff"), n(0.2)]),
        ("saturate", vec![text("#806040"), n(0.5)]),
        ("desaturate", vec![text("#806040"), n(1.)]),
        ("invert", vec![text("#00ff8040")]),
        ("contrast", vec![text("#000000"), text("#ffffff")]),
        ("contrast", vec![text("#777777"), text("#777777")]),
        ("mix", vec![text("#000000"), text("#ffffff")]),
        ("mix", vec![text("#000000"), text("#ffffff"), n(0.25)]),
        ("mix", vec![text("#00000000"), text("#ffffffff"), n(2.)]),
        ("lighten", vec![text("not a colour"), n(0.1)]),
        ("lighten", vec![text("#12"), n(0.1)]),
    ]
}

fn record(out: &mut String, name: &str, arguments: Vec<Value>) {
    let shown: Vec<String> = arguments.iter().map(|value| format!("{value:?}")).collect();
    let call = Value::Expression(Box::new(Expression::Call(name.into(), arguments)));
    let result = match call.evaluate(&Properties::new(), &Properties::new()) {
        Ok(value) => format!("{value:?}"),
        Err(error) => format!("ERROR {error}"),
    };
    writeln!(out, "{name}({}) => {result}", shown.join(", ")).unwrap();
}

#[test]
fn every_builtin_matches_its_recorded_behaviour() {
    let pool = pool();
    // Two-argument sweeps use a spread of kinds; three-argument sweeps a smaller one, to keep the
    // record readable while still crossing numbers, text, lists and null.
    let pair: Vec<&Value> = [0, 1, 3, 5, 6, 8, 10].iter().map(|&at| &pool[at]).collect();
    let triple: Vec<&Value> = [0, 3, 6, 10].iter().map(|&at| &pool[at]).collect();
    let mut out = String::new();
    for name in BUILTINS {
        record(&mut out, name, vec![]);
        for a in &pool {
            record(&mut out, name, vec![a.clone()]);
        }
        for a in &pair {
            for b in &pair {
                record(&mut out, name, vec![(*a).clone(), (*b).clone()]);
            }
        }
        for a in &triple {
            for b in &triple {
                for c in &triple {
                    record(
                        &mut out,
                        name,
                        vec![(*a).clone(), (*b).clone(), (*c).clone()],
                    );
                }
            }
        }
    }
    for (name, arguments) in curated() {
        record(&mut out, name, arguments);
    }
    golden::assert_golden("eval_builtins.txt", &out);
}
