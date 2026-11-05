# Shared native scene snapshots: implementation boundary

Status: first shared-storage implementation, 2026-11-03; broader qualification pending.

The current public `SceneControl.state: Properties` owns a complete scope.
`scene_expand` materializes it into `Node.props.__state` for each control.
Visual templates and Slider endpoints can read that snapshot; only after their
last evaluation does `render_scene_with_extents` transfer it into SceneControl.
`compact_scene_control_state` removes duplicated lexical replay metadata, but
retains model fields. Moving the final map avoids a second copy, not the first.

Runtime retains these controls in `ui_binding_scene`. Binding transactions read
the live root from `Document.states`, but need captured locals, stable keys,
parent frames, lexical replay and key-expression scope from displayed controls.
`resolve_binding_item_inner` additionally preserves exact dotted-field lookup
precedence and merges missing captured locals into its evaluator scope. A
root-field filter without these rules would change resolution semantics.

## Required representation change

Introduce an internal expanded-control representation carrying shared immutable
scope ownership and a borrowed property view. Keep public `SceneControl` and
`SceneArtifact` owned and serializable. Materialize public state only at export;
Runtime should retain shared scopes directly. Sharing must begin at expansion,
before the current `state.into_owned()` call, rather than converting an already
cloned map into Arc afterward. Local/Let/For overlays must preserve shadowing and
snapshot lifetime across subsequent model replacement.

Visual instantiation must accept this scope without inserting another owned
`Value::Object` into props merely to evaluate `props.__state`. Existing Value
and Properties public types need not change. Resolve the borrowed state view at
the evaluator boundary; propagate it through property/contract/group/template
evaluation. Reuse that same representation in `binding_scene_controls` and in
retained Runtime metadata, so failed-render recovery does not reintroduce copies.

Public export remains intentionally proportional to serialized output size;
it cannot claim O(1) per-control transport when full state is requested.

## Acceptance gates

- Public artifact JSON and state snapshots remain identical for existing scenes.
- Template `props.__state`, Slider endpoint expressions and local/lexical
  shadowing remain correct; exact dotted-field and enum precedence retained.
- Retained snapshots survive mutation/reorder/replacement of Document.states.
- Binding transactions, duplicate keys, failed render recovery, old displayed
  targets and callback-triggered replacement preserve existing behavior.
- Internal initial render and edit/rebuild allocation measurements cover
  8/32/96/256 controls and increasing unrelated root payload independently.
  Demonstrate shared-root payload is not copied once per control internally.
- Re-run native suite, fresh WASM Node/browser, public export compatibility,
  and paired release timing/RSS. Serialized JSON size is not RSS evidence.

Reproduction: `snapshot_storage_probe` quantifies the current public duplication
and asserts root-state immutability. It is a baseline probe, not a performance
regression budget or proof that the representation change is complete.

## First-stage representation (historical)

`Properties` is now a value-semantic newtype over `Arc<BTreeMap<String, Value>>`.
Clone shares immutable map storage. Mutable access detaches through
`Arc::make_mut`; nested object maps use the same representation. Existing
`Value::Object`, Node and SceneControl data shapes remain intact, avoiding a
second private-handle protocol in `props.__state`. Public JSON serializes the
underlying map exactly as before, with no Arc IDs or sharing metadata.

Sibling controls sharing an unchanged root now retain the same map allocation.
Mutation of a control snapshot or Document.states detaches it, preserving value
semantics and old snapshots. Owned iteration detaches if necessary. Shared maps
that are extended/mutated still clone their entries; list values remain owned
Vecs. Thus local/For overlays and mutable wide roots still need measurement.

Rust API migration: Properties used to be a type alias. Construct with
`Properties::new()`, `Properties::from([...])`, iterator collection, or convert
existing `BTreeMap<String,Value>` with `.into()`. Read/write map methods work via
Deref/DerefMut. JSON schema unchanged. Export remains proportional to output.
The earlier internal-view design is no longer required for unchanged whole-map
sharing; evaluator views may still help overlays that currently detach maps.

## Evaluation scope overlays — 2026-11-03

Ordinary model maps remain flat Arc/BTreeMap COW. Private evaluation scopes
share an immutable base plus ordered deltas/tombstones. Captured outer locals
are shared between For frames. Delta values detach only on mutation; old
snapshots preserve value semantics. Small loops/scopes use bounded flat copies.
Lookups and serialization visit visible values; explicit BTreeMap views and
mutable whole-map iteration materialize. Owned materialization skips masked
base values. `Value::List` remains an owned Vec; evaluation, root detachment
and export can still copy. Ordinary model replacement releases previous maps
after their last snapshots drop.

Borrowed iteration now exposes `PropertyIter`; callers with explicit standard
`btree_map::Iter` annotations must migrate. JSON and Value variants are unchanged.

[Qualification](benchmarks/property-overlay-qualification-2026-11-03.json):
426 exact frames, 50 exact artifact JSON, Node14, Chromium19, Metal Reveal126
and fresh native256 GPU→CPU→GPU model/scroll smoke passed. Retained artifact
bytes256:5,104,326→961,772; initial live Runtime:6,823,252→2,681,002.
Allocator accounting is not RSS. Paired instrumented edit timings exclude
paint/presentation; small8/32 controls show approximately1–5% overhead.
Long lifecycle/RSS, native input latency and other platforms remain open.
