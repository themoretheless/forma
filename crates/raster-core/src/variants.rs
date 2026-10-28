//! Textual shader variants derived from the single-object shaders. These are
//! ports of the transforms the browser renderer applies in TypeScript; both
//! pin structural anchors and panic when a shader module drifts from the
//! contract, so a broken composition fails at pipeline creation, not draw
//! time.

/// Per-draw style without a uniform buffer rewrite: `ob.style` is served
/// from the `immediate_address_space` instead of the Obj uniform.
pub fn immediate_object_shader(source: &str) -> String {
    assert!(
        source.contains("var<uniform> ob: Obj;"),
        "Immediate shader contract changed: missing object uniform"
    );
    let prefix = "requires immediate_address_space;\nvar<immediate> im_style: vec4f;\nfn objectStyle() -> vec4f { return im_style; }\n";
    prefix.to_string() + &source.replace("ob.style", "objectStyle()")
}

/// Vertex-output struct marker of an object shader family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexOutput {
    /// Mesh and deep-mesh shaders.
    V,
    /// Edge shader.
    EdgeV,
}

impl VertexOutput {
    fn as_str(self) -> &'static str {
        match self {
            VertexOutput::V => "V",
            VertexOutput::EdgeV => "EdgeV",
        }
    }
}

/// Per-instance Obj records from a read-only storage buffer.
pub fn instanced_object_shader(source: &str, output: VertexOutput) -> String {
    let output = output.as_str();
    let replacements: [(&str, &str); 6] = [
        ("var<uniform> ob: Obj;", "var<storage, read> objects: array<Obj>;"),
        (
            &format!("struct {output} {{"),
            &format!("struct {output} {{ @location(2) @interpolate(flat) instance: u32,"),
        ),
        ("@vertex fn vs(", "@vertex fn vs(@builtin(instance_index) instance: u32, "),
        (
            &format!(" -> {output} {{"),
            &format!(" -> {output} {{\n  let ob = objects[instance];"),
        ),
        (&format!("return {output}("), &format!("return {output}(instance, ")),
        (
            &format!("@fragment fn fs(v: {output}) -> @location(0) vec4f {{"),
            &format!("@fragment fn fs(v: {output}) -> @location(0) vec4f {{\n  let ob = objects[v.instance];"),
        ),
    ];
    let mut source = source.to_string();
    for (from, to) in replacements {
        assert!(
            source.split(from).count() == 2,
            "Instanced shader contract changed: {from}"
        );
        source = source.replacen(from, to, 1);
    }
    source
}
