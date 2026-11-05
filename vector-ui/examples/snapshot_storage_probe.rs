//! Quantify owned control-state duplication without treating serialized bytes as RSS.
use forma::form_document::{Document, Properties, Value};
use std::collections::BTreeMap;
fn main() {
    let mut rows = Vec::new();
    for count in [8, 32, 96, 256] {
        let inputs = (0..count).map(|i| format!("TextInput {{ key:'field-{i}'; width:100; height:30; value <-> state.query; }}")).collect::<String>();
        let files = BTreeMap::from([("App.ui".into(), format!("component App {{ state query: String = 'seed'; state payload: List = []; Column {{ {inputs} }} }}"))]);
        let payload = Value::List((0..256).map(|i| Value::Object(Properties::from([("id".into(), Value::Text(format!("record-{i}"))), ("data".into(), Value::Text("unrelated-payload".repeat(8)))]))).collect());
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::from([("payload".into(), payload)])).unwrap();
        let root_before = doc.state_json().unwrap();
        let artifact = doc.render_scene_artifact().unwrap();
        assert_eq!(artifact.controls.len(), count);
        let repeated_records: usize = artifact.controls.iter().map(|c| {
            let Value::List(payload) = &c.state["payload"] else { panic!("public root snapshot") };
            payload.len()
        }).sum();
        let json = artifact.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let control_bytes = serde_json::to_vec(&parsed["controls"]).unwrap().len();
        assert_eq!(doc.state_json().unwrap(), root_before);
        rows.push(serde_json::json!({"controls":count,"root_json_bytes":root_before.len(),"artifact_json_bytes":json.len(),"controls_json_bytes":control_bytes,"payload_records_per_root":256,"retained_payload_records":repeated_records}));
    }
    println!("{}", serde_json::json!({"scope":"Public owned SceneArtifact duplication for independent controls sharing one root; serialized bytes are not allocator/RSS measurements","rows":rows}));
}
