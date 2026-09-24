//! Content-keyed template parse cache. A repeated (template text, spec) pair must reuse
//! the parsed Template; any difference in either half of the key must not.
use forma::markup::ButtonSpec;
use forma::{template, Runtime};
use std::sync::Arc;

const SOURCE: &str = "component Button { Rectangle { radius: props.radius; background: Brush { color: props.background; hover: #a8baff; transition: 140ms; }; Border { width: 1; background: Brush { color: #bed0ff; transition: 100ms; }; } Text { text: props.text; color: props.color; font.size: props.font.size; } PointerArea { clicked -> events.clicked(); } } }";

fn spec(text: &str) -> ButtonSpec {
    ButtonSpec { text: text.into(), key: "run".into(), ..ButtonSpec::default() }
}

#[test]
fn a_reused_template_equals_a_fresh_parse() {
    let props = spec("Найти");
    let first = template::parse_cached(SOURCE, &props).unwrap();
    let second = template::parse_cached(SOURCE, &props).unwrap();
    assert_eq!(first, second);
    assert_eq!(*second, template::parse(SOURCE, &props).unwrap());
}

#[test]
fn a_different_spec_under_the_same_text_is_parsed_again() {
    let _ = template::parse_cached(SOURCE, &spec("Найти")).unwrap();
    let second = template::parse_cached(SOURCE, &spec("Отмена")).unwrap();
    assert_eq!(second.text.as_ref().unwrap().text, "Отмена");
    assert_eq!(*second, template::parse(SOURCE, &spec("Отмена")).unwrap());
}

#[test]
fn a_different_text_under_the_same_spec_is_parsed_again() {
    let props = spec("Найти");
    let plain = template::parse_cached(SOURCE, &props).unwrap();
    let edited = SOURCE.replace("hover: #a8baff", "hover: #ff00aa");
    let changed = template::parse_cached(&edited, &props).unwrap();
    assert_eq!(plain.fill.as_ref().unwrap().hover, Some([168, 186, 255, 255]));
    assert_eq!(changed.fill.as_ref().unwrap().hover, Some([255, 0, 170, 255]));
    assert_eq!(*changed, template::parse(&edited, &props).unwrap());
}

#[test]
fn a_hit_shares_one_template_and_a_writer_cannot_poison_it() {
    let props = spec("Найти");
    let mut first = template::parse_cached(SOURCE, &props).unwrap();
    // The point of the cache: a repeated key hands out the same allocation, not a copy.
    assert!(Arc::ptr_eq(&first, &template::parse_cached(SOURCE, &props).unwrap()));
    let damaged = Arc::make_mut(&mut first);
    damaged.content.clear();
    damaged.text = None;
    damaged.radius = 0.;
    assert_ne!(damaged.radius, template::parse(SOURCE, &props).unwrap().radius);
    let second = template::parse_cached(SOURCE, &props).unwrap();
    assert_eq!(*second, template::parse(SOURCE, &props).unwrap());
}

#[test]
fn failed_parses_are_not_cached() {
    let broken = SOURCE.replace("radius: props.radius", "radius: props.missing");
    let props = spec("Найти");
    let before = template::parse_cache_stats();
    assert!(template::parse_cached(&broken, &props).is_err());
    assert!(template::parse_cached(&broken, &props).is_err());
    let after = template::parse_cache_stats();
    assert_eq!(after.hits, before.hits);
    assert_eq!(after.entries, before.entries);
}

#[test]
fn controls_sharing_a_component_keep_their_own_specs() {
    // Every control of a scene parses the same component text, so the spec alone has to
    // separate the cache entries.
    let scene = "component Demo { Frame { width:200; height:200; \
        Button { key:'a'; text:'Первый'; radius:4; } \
        Button { key:'b'; text:'Второй'; radius:12; } } }";
    let runtime = Runtime::from_sources(scene, SOURCE).unwrap();
    assert_eq!(runtime.control_count(), 2);
    assert_eq!(runtime.control_label(0), "Первый");
    assert_eq!(runtime.control_label(1), "Второй");
    let reloaded = Runtime::from_sources(scene, SOURCE).unwrap();
    assert_eq!(reloaded.control_bounds(0), runtime.control_bounds(0));
    assert_eq!(reloaded.control_label(1), "Второй");
}

#[test]
fn the_cache_stays_inside_its_entry_budget() {
    template::flush_parse_cache();
    for index in 0..(template::MAX_PARSED_TEMPLATES + 200) {
        let text = format!("control {index}");
        let reused = template::parse_cached(SOURCE, &spec(&text)).unwrap();
        assert_eq!(reused.text.as_ref().unwrap().text, text);
    }
    let stats = template::parse_cache_stats();
    assert!(stats.entries <= template::MAX_PARSED_TEMPLATES, "entries grew past the cap: {}", stats.entries);
    assert!(stats.bytes <= template::MAX_PARSED_BYTES, "weight grew past the cap: {}", stats.bytes);
    assert!(stats.entries > template::MAX_PARSED_TEMPLATES / 2, "evicted far too early: {}", stats.entries);
}
