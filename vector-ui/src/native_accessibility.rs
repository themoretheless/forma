//! Native AccessKit tree translation. Window adapter integration owns delivery.
use accesskit::{Action,Node,NodeId,Rect,Role,TreeId,TreeInfo,TreeUpdate};
use crate::runtime::{A11yRole,A11yTree};

fn id(value:usize)->NodeId {NodeId(value as u64)}
struct TextRun<'a>{id:NodeId,value:&'a str,lengths:Vec<u8>,offsets:Vec<u32>}
fn text_runs(parent:usize,value:&str)->Option<Vec<TextRun<'_>>> {
    if parent>0x7fff_ffff{return None;}
    let mut result=Vec::new();let mut offset=0;
    let mut lines:Vec<&str>=value.split_inclusive('\n').collect();
    if value.is_empty()||value.ends_with('\n'){lines.push("");}
    for (line,value) in lines.into_iter().enumerate(){
        let line=u32::try_from(line).ok()?;
        let id=NodeId((1u64<<63)|((parent as u64)<<32)|u64::from(line));
        let mut lengths=Vec::new();let mut offsets=vec![offset];
        for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(value,true){
            lengths.push(u8::try_from(grapheme.len()).ok()?);
            offset+=grapheme.encode_utf16().count() as u32;offsets.push(offset);
        }
        result.push(TextRun{id,value,lengths,offsets});
    }
    Some(result)
}
fn text_position(runs:&[TextRun<'_>],offset:u32)->accesskit::TextPosition {
    let run=runs.iter().find(|run|offset<*run.offsets.last().unwrap()).unwrap_or_else(||runs.last().unwrap());
    let index=run.offsets.partition_point(|at|*at<offset).min(run.lengths.len());
    accesskit::TextPosition{node:run.id,character_index:index}
}


/// Translate a complete semantic snapshot. Coordinates remain in Runtime units;
/// the window host must supply its logical-to-window transform when delivering.
/// Text input children expose grapheme units through per-line AccessKit text runs.
/// A grapheme exceeding the u8 byte-length format disables text-run selection.
pub fn tree_update(tree:&A11yTree)->TreeUpdate {
    let mut text_nodes=Vec::new();
    let mut nodes:Vec<_>=tree.nodes.iter().map(|source| {
        let role=match source.role {
            A11yRole::Window=>Role::Window,
            A11yRole::ScrollView=>Role::ScrollView,
            A11yRole::Dialog=>Role::Dialog,
            A11yRole::Button=>Role::Button,
            A11yRole::TextInput=>if source.multiline{Role::MultilineTextInput}else{Role::TextInput},
            A11yRole::Slider=>Role::Slider,
            A11yRole::StaticText=>Role::Label,
        };
        let mut node=Node::new(role);
        node.set_children(source.children.iter().copied().map(id).collect::<Vec<_>>());
        if source.role==A11yRole::StaticText {node.set_value(source.label.clone());}
        else {node.set_label(source.label.clone());}
        if let Some(value)=&source.value {node.set_value(value.clone());}
        if let Some(value)=source.numeric_value {node.set_numeric_value(f64::from(value));}
        if source.disabled||source.modal_blocked {node.set_disabled();}
        if source.read_only {node.set_read_only();}
        let [x,y,w,h]=source.bounds.map(f64::from);
        node.set_bounds(Rect::new(x,y,x+w,y+h));
        if source.actions.contains(&"focus"){node.add_action(Action::Focus);}
        if source.actions.contains(&"click"){node.add_action(Action::Click);}
        if source.role==A11yRole::TextInput&&source.actions.contains(&"set_value"){node.add_action(Action::SetValue);}
        if source.role==A11yRole::Slider {
            node.set_min_numeric_value(0.);
            node.set_max_numeric_value(1.);
            node.set_numeric_value_step(0.01);
            node.set_numeric_value_jump(0.1);
            if source.actions.contains(&"increment"){node.add_action(Action::Increment);}
            if source.actions.contains(&"decrement"){node.add_action(Action::Decrement);}
            if source.actions.contains(&"set_value"){node.add_action(Action::SetValue);}
        }
        if source.role==A11yRole::ScrollView&&source.actions.contains(&"scroll")&&!source.modal_blocked {
            for action in [Action::ScrollUp,Action::ScrollDown,Action::ScrollLeft,Action::ScrollRight] {
                node.add_action(action);
            }
        }
        if source.role==A11yRole::Dialog {
            if !source.disabled {node.set_live(accesskit::Live::Polite);}
            if source.actions.contains(&"dismiss")&&!source.modal_blocked {
                node.add_action(Action::Collapse);
            }
        }
        if source.role==A11yRole::TextInput {
            if let Some(runs)=source.value.as_deref().and_then(|value|text_runs(source.id,value)) {
                node.set_children(runs.iter().map(|run|run.id).collect::<Vec<_>>());
                if let Some([anchor,focus])=source.selection_utf16 {
                    node.set_text_selection(accesskit::TextSelection{anchor:text_position(&runs,anchor),focus:text_position(&runs,focus)});
                }
                if source.actions.contains(&"set_text_selection"){node.add_action(Action::SetTextSelection);}
                for run in runs {let mut text=Node::new(Role::TextRun);text.set_value(run.value);text.set_character_lengths(run.lengths);text_nodes.push((run.id,text));}
            }
        }
        (id(source.id),node)
    }).collect();
    nodes.extend(text_nodes);
    TreeUpdate{nodes,tree:Some(TreeInfo::new(id(tree.root_id))),tree_id:TreeId::ROOT,focus:id(tree.focused_id.unwrap_or(tree.root_id))}
}

/// Route basic native actions through the same Runtime admission as keyboard.
/// Range actions need additional metadata before admission.
pub fn apply_action(runtime:&mut crate::Runtime,target:NodeId,action:Action)->bool {
    apply_action_data(runtime,target,action,None)
}

pub fn apply_action_data(runtime:&mut crate::Runtime,target:NodeId,action:Action,data:Option<&accesskit::ActionData>)->bool {
    let Ok(node_id)=usize::try_from(target.0) else{return false;};
    if action==Action::SetTextSelection {
        let Some(accesskit::ActionData::SetTextSelection(selection))=data else{return false;};
        let tree=runtime.accessibility_tree();
        let Some(node)=tree.nodes.iter().find(|node|node.id==node_id&&node.actions.contains(&"set_text_selection")) else{return false;};
        let Some(runs)=node.value.as_deref().and_then(|value|text_runs(node_id,value)) else{return false;};
        let offset=|position:accesskit::TextPosition|runs.iter().find(|run|run.id==position.node).and_then(|run|run.offsets.get(position.character_index)).copied();
        let (Some(anchor),Some(focus))=(offset(selection.anchor),offset(selection.focus)) else{return false;};
        return runtime.accessibility_set_text_selection(node_id,anchor,focus);
    }
    if action==Action::SetValue {
        let tree=runtime.accessibility_tree();
        let Some(node)=tree.nodes.iter().find(|node|node.id==node_id&&node.actions.contains(&"set_value")) else{return false;};
        return match (node.role,data) {
            (A11yRole::TextInput,Some(accesskit::ActionData::Value(value)))=>runtime.accessibility_set_text_value(node_id,value),
            (A11yRole::Slider,Some(accesskit::ActionData::NumericValue(value)))=>runtime.accessibility_set_range_value(node_id,*value),
            (A11yRole::Slider,Some(accesskit::ActionData::Value(value)))=>{
                let trimmed=value.trim().trim_end_matches('%').trim();
                let Ok(parsed)=trimmed.parse::<f64>() else{return false;};
                runtime.accessibility_set_range_value(node_id,parsed)
            }
            _=>false,
        };
    }
    if matches!(action,Action::Increment|Action::Decrement) {
        let name=if action==Action::Increment{"increment"}else{"decrement"};
        let tree=runtime.accessibility_tree();
        if !tree.nodes.iter().any(|node|node.id==node_id&&node.role==A11yRole::Slider&&node.actions.contains(&name)){return false;}
        return runtime.accessibility_step_range(node_id,if action==Action::Increment{0.05}else{-0.05});
    }
    if matches!(action,Action::ScrollUp|Action::ScrollDown|Action::ScrollLeft|Action::ScrollRight) {
        let tree=runtime.accessibility_tree();
        if !tree.nodes.iter().any(|node|node.id==node_id&&node.role==A11yRole::ScrollView&&node.actions.contains(&"scroll")&&!node.modal_blocked){return false;}
        let (dx,dy)=match action {
            Action::ScrollUp=>(0.,-48.),
            Action::ScrollDown=>(0.,48.),
            Action::ScrollLeft=>(-48.,0.),
            Action::ScrollRight=>(48.,0.),
            _=>unreachable!(),
        };
        return runtime.accessibility_scroll(node_id,dx,dy);
    }
    if action==Action::Collapse {
        let tree=runtime.accessibility_tree();
        if !tree.nodes.iter().any(|node|node.id==node_id&&node.role==A11yRole::Dialog&&node.actions.contains(&"dismiss")&&!node.modal_blocked){return false;}
        return runtime.accessibility_dismiss_modal(node_id);
    }
    let name=match action {Action::Focus=>"focus",Action::Click=>"click",_=>return false};
    if !runtime.accessibility_tree().nodes.iter().any(|node|node.id==node_id&&node.actions.contains(&name)){return false;}

    let Some(index)=runtime.nodes().get(node_id).and_then(|node|node.control) else{return false;};
    match action {
        Action::Focus=>runtime.focus_control(index),
        Action::Click=>{if !runtime.focus_control(index){return false;}runtime.activate_control(index);true},
        _=>false,
    }
}

/// Window-lifetime IDs prevent queued OS actions from using recycled scene IDs.
#[derive(Default)]
pub struct PublishedTree {
    scene:Option<u64>,next:u64,
    last_snapshot:Option<A11yTree>,last_scale:f64,
    ids:std::collections::BTreeMap<NodeId,NodeId>,
    reverse:std::collections::BTreeMap<NodeId,NodeId>,
    text_values:std::collections::BTreeMap<usize,String>,
}
impl PublishedTree {
    pub fn invalidate(&mut self){self.last_snapshot=None;}
    pub fn update_if_changed(&mut self,runtime:&crate::Runtime,scale:f64)->Result<Option<TreeUpdate>,String> {
        if !scale.is_finite()||scale<=0. {return Err("Invalid accessibility scale".into());}
        let snapshot=runtime.accessibility_tree();
        if self.scene==Some(runtime.native_scene_identity())&&self.last_scale==scale&&self.last_snapshot.as_ref()==Some(&snapshot){return Ok(None);}
        let mut update=self.update_snapshot(runtime.native_scene_identity(),&snapshot)?;
        let root=update.tree.as_ref().unwrap().root;
        update.nodes.iter_mut().find(|(id,_)|*id==root).unwrap().1.set_transform(accesskit::Affine::scale(scale));
        self.last_snapshot=Some(snapshot);self.last_scale=scale;Ok(Some(update))
    }
    pub fn update(&mut self,runtime:&crate::Runtime)->Result<TreeUpdate,String> {
        self.invalidate();
        self.update_snapshot(runtime.native_scene_identity(),&runtime.accessibility_tree())
    }
    fn update_snapshot(&mut self,scene:u64,snapshot:&A11yTree)->Result<TreeUpdate,String> {
        if self.scene!=Some(scene) {
            self.ids.clear();self.reverse.clear();self.text_values.clear();
        }
        // Text-position IDs describe one value, not merely one line number.
        for node in &snapshot.nodes {
            if node.role==A11yRole::TextInput {
                if self.text_values.get(&node.id)!=node.value.as_ref() {
                    self.ids.retain(|key,_|key.0>>63==0||((key.0&!(1u64<<63))>>32)!=node.id as u64);
                }
            }
        }
        self.text_values=snapshot.nodes.iter().filter(|node|node.role==A11yRole::TextInput).filter_map(|node|node.value.clone().map(|value|(node.id,value))).collect();
        let mut update=tree_update(snapshot);
        let live:std::collections::BTreeSet<_>=update.nodes.iter().map(|(key,_)|*key).collect();
        self.ids.retain(|key,_|live.contains(key));
        for (key,_) in &update.nodes {
            if !self.ids.contains_key(key) {
                let native=if key.0==snapshot.root_id as u64{NodeId(0)}else{
                    self.next=self.next.checked_add(1).ok_or_else(||"AccessKit node IDs exhausted".to_owned())?;NodeId(self.next)
                };
                self.ids.insert(*key,native);
            }
        }
        self.reverse=self.ids.iter().map(|(local,native)|(*native,*local)).collect();
        for (key,node) in &mut update.nodes {
            *key=self.ids[key];
            node.set_children(node.children().iter().map(|key|self.ids[key]).collect::<Vec<_>>());
            if let Some(selection)=node.text_selection().copied() {
                node.set_text_selection(accesskit::TextSelection{
                    anchor:accesskit::TextPosition{node:self.ids[&selection.anchor.node],..selection.anchor},
                    focus:accesskit::TextPosition{node:self.ids[&selection.focus.node],..selection.focus},
                });
            }
        }
        update.focus=self.ids[&update.focus];
        if let Some(tree)=&mut update.tree {tree.root=self.ids[&tree.root];}
        self.scene=Some(scene);Ok(update)
    }
    pub fn apply(&self,runtime:&mut crate::Runtime,request:&accesskit::ActionRequest)->bool {
        if self.scene!=Some(runtime.native_scene_identity())||request.target_tree!=TreeId::ROOT{return false;}
        let Some(target)=self.reverse.get(&request.target_node).copied() else{return false;};
        let mut data=request.data.clone();
        if let Some(accesskit::ActionData::SetTextSelection(selection))=&mut data {
            let snapshot=runtime.accessibility_tree();
            let Some(node)=snapshot.nodes.iter().find(|node|node.id as u64==target.0) else{return false;};
            if self.text_values.get(&node.id)!=node.value.as_ref(){return false;}
            let (Some(anchor),Some(focus))=(self.reverse.get(&selection.anchor.node),self.reverse.get(&selection.focus.node)) else{return false;};
            selection.anchor.node=*anchor;selection.focus.node=*focus;
        }
        apply_action_data(runtime,target,request.action,data.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translates_real_editor_metadata_and_refuses_unadvertised_actions() {
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { Column { TextInput { key:'ref'; label:'Reference'; value:'你好🙂'; readOnly:true; multiline:true; } TextInput { key:'disabled'; disabled:true; } } }".into())]);
        let mut runtime=crate::Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
        let semantic=runtime.accessibility_tree();let reference=semantic.nodes.iter().find(|n|n.key=="ref").unwrap();
        let disabled=semantic.nodes.iter().find(|n|n.key=="disabled").unwrap();
        let update=tree_update(&semantic);assert_eq!(update.focus,id(semantic.root_id));
        let node=&update.nodes.iter().find(|(key,_)|*key==id(reference.id)).unwrap().1;
        assert_eq!(node.role(),Role::MultilineTextInput);assert_eq!(node.label(),Some("Reference"));
        assert_eq!(node.value(),Some("你好🙂"));assert!(node.is_read_only());assert!(node.supports_action(Action::Focus));
        assert!(node.supports_action(Action::SetTextSelection));
        assert!(!apply_action(&mut runtime,id(reference.id),Action::Click));
        assert!(!apply_action(&mut runtime,id(disabled.id),Action::Focus));
        assert!(apply_action(&mut runtime,id(reference.id),Action::Focus));
        assert_eq!(tree_update(&runtime.accessibility_tree()).focus,id(reference.id));
        assert!(!apply_action(&mut runtime,NodeId(u64::MAX),Action::Focus));
    }
    #[test]
    fn set_value_requires_text_payload_and_preserves_readonly() {
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } TextInput { key:'locked'; value:'reference'; readOnly:true; } } }".into())]);
        let mut runtime=crate::Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
        let semantic=runtime.accessibility_tree();let editor=semantic.nodes.iter().find(|n|n.key=="editor").unwrap().id;
        let locked=semantic.nodes.iter().find(|n|n.key=="locked").unwrap().id;
        let update=tree_update(&semantic);
        assert!(update.nodes.iter().find(|(key,_)|*key==id(editor)).unwrap().1.supports_action(Action::SetValue));
        assert!(!update.nodes.iter().find(|(key,_)|*key==id(locked)).unwrap().1.supports_action(Action::SetValue));
        assert!(!apply_action(&mut runtime,id(editor),Action::SetValue));
        assert!(!apply_action_data(&mut runtime,id(editor),Action::SetValue,Some(&accesskit::ActionData::NumericValue(2.))));
        let payload=accesskit::ActionData::Value("你好\r\n🙂".into());
        assert!(!apply_action_data(&mut runtime,id(locked),Action::SetValue,Some(&payload)));
        assert!(apply_action_data(&mut runtime,id(editor),Action::SetValue,Some(&payload)));
        assert_eq!(runtime.text_value(0),"你好 🙂");runtime.sync_ui_bindings_native().unwrap();
        let state:serde_json::Value=serde_json::from_str(&runtime.ui_state_json()).unwrap();assert_eq!(state["query"],"你好 🙂");
        runtime.text_key("z",false,true);assert_eq!(runtime.text_value(0),"seed");assert_eq!(runtime.text_value(1),"reference");
    }

    #[test]
    fn text_runs_roundtrip_graphemes_crlf_reverse_and_trailing_empty_line() {
        let files:std::collections::BTreeMap<String,String>=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = ''; Column { TextInput { key:'editor'; multiline:true; readOnly:true; value <-> state.query; } } }".into())]);
        let mut runtime=crate::Runtime::new();runtime.load_ui_files_json_native(&serde_json::to_string(&files).unwrap(),"App.ui",&serde_json::json!({"query":"A😀\r\né\n"}).to_string()).unwrap();
        let semantic=runtime.accessibility_tree();let editor=semantic.nodes.iter().find(|n|n.key=="editor").unwrap();let target=id(editor.id);
        let update=tree_update(&semantic);let parent=&update.nodes.iter().find(|(key,_)|*key==target).unwrap().1;
        let children=parent.children();assert_eq!(children.len(),3);
        let first=&update.nodes.iter().find(|(key,_)|*key==children[0]).unwrap().1;
        let second=&update.nodes.iter().find(|(key,_)|*key==children[1]).unwrap().1;
        assert_eq!(first.value(),Some("A😀\r\n"));assert_eq!(first.character_lengths(),&[1,4,2]);
        assert_eq!(second.character_lengths(),&[3,1]);
        let selection=accesskit::TextSelection{anchor:accesskit::TextPosition{node:children[1],character_index:1},focus:accesskit::TextPosition{node:children[0],character_index:1}};
        assert!(apply_action_data(&mut runtime,target,Action::SetTextSelection,Some(&accesskit::ActionData::SetTextSelection(selection))));
        assert_eq!(runtime.text_selected(),"😀\r\né");assert_eq!(runtime.text_selection_utf16(),vec![6,1]);
        let roundtrip=tree_update(&runtime.accessibility_tree());assert_eq!(roundtrip.nodes.iter().find(|(key,_)|*key==target).unwrap().1.text_selection(),Some(&selection));
        let invalid=accesskit::TextSelection{anchor:accesskit::TextPosition{node:children[1],character_index:999},focus:selection.focus};
        assert!(!apply_action_data(&mut runtime,target,Action::SetTextSelection,Some(&accesskit::ActionData::SetTextSelection(invalid))));
        let end=accesskit::TextPosition{node:children[2],character_index:0};
        assert!(apply_action_data(&mut runtime,target,Action::SetTextSelection,Some(&accesskit::ActionData::SetTextSelection(accesskit::TextSelection{anchor:end,focus:end}))));
        assert_eq!(runtime.text_selection_utf16(),vec![7,7]);
    }
    #[test]
    fn oversized_grapheme_disables_unrepresentable_text_run_selection() {
        let value=format!("x{}","\u{301}".repeat(300));assert!(text_runs(1,&value).is_none());
        let files:std::collections::BTreeMap<String,String>=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = ''; Column { TextInput { key:'editor'; value <-> state.query; } } }".into())]);
        let mut runtime=crate::Runtime::new();runtime.load_ui_files_json_native(&serde_json::to_string(&files).unwrap(),"App.ui",&serde_json::json!({"query":value}).to_string()).unwrap();
        let tree=runtime.accessibility_tree();let target=id(tree.nodes.iter().find(|n|n.key=="editor").unwrap().id);
        let update=tree_update(&tree);let node=&update.nodes.iter().find(|(key,_)|*key==target).unwrap().1;
        assert!(!node.supports_action(Action::SetTextSelection));assert!(node.children().is_empty());assert!(node.supports_action(Action::SetValue));
    }

    #[test]
    fn published_ids_reject_retired_scene_even_when_local_ids_and_keys_match() {
        let files=serde_json::json!({"App.ui":"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } } }"}).to_string();
        let mut runtime=crate::Runtime::new();runtime.load_ui_files_json_native(&files,"App.ui","{}").unwrap();
        let mut published=PublishedTree::default();let first=published.update(&runtime).unwrap();
        let target=first.nodes.iter().find(|(_,node)|node.role()==Role::TextInput).unwrap().0;
        let request=accesskit::ActionRequest{action:Action::SetValue,target_tree:TreeId::ROOT,target_node:target,data:Some(accesskit::ActionData::Value("stale".into()))};
        let unchanged=published.update(&runtime).unwrap();assert!(unchanged.nodes.iter().any(|(id,_)|*id==target));
        runtime.load_ui_files_json_native(&files,"App.ui","{\"query\":\"replacement\"}").unwrap();
        assert!(!published.apply(&mut runtime,&request),"reject even before new tree publication");
        let next=published.update(&runtime).unwrap();let new_target=next.nodes.iter().find(|(_,node)|node.role()==Role::TextInput).unwrap().0;
        assert_ne!(target,new_target);assert!(!published.apply(&mut runtime,&request));assert_eq!(runtime.text_value(0),"replacement");
        let fresh=accesskit::ActionRequest{target_node:new_target,..request};assert!(published.apply(&mut runtime,&fresh));assert_eq!(runtime.text_value(0),"stale");
    }
    #[test]
    fn published_text_positions_retire_when_text_changes_before_and_after_publication() {
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { Column { TextInput { key:'editor'; value:'seed'; } } }".into())]);
        let mut runtime=crate::Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
        let mut published=PublishedTree::default();let first=published.update(&runtime).unwrap();
        let (target,parent)=first.nodes.iter().find(|(_,node)|node.role()==Role::TextInput).unwrap();let run=parent.children()[0];
        let position=accesskit::TextPosition{node:run,character_index:1};
        let request=accesskit::ActionRequest{action:Action::SetTextSelection,target_tree:TreeId::ROOT,target_node:*target,data:Some(accesskit::ActionData::SetTextSelection(accesskit::TextSelection{anchor:position,focus:position}))};
        assert!(published.apply(&mut runtime,&request));runtime.text_insert("X");
        assert!(!published.apply(&mut runtime,&request),"old positions cannot index unpublished new text");
        let next=published.update(&runtime).unwrap();let (new_target,new_parent)=next.nodes.iter().find(|(_,node)|node.role()==Role::TextInput).unwrap();
        assert_eq!(target,new_target);assert_ne!(run,new_parent.children()[0]);assert!(!published.apply(&mut runtime,&request));
        let position=accesskit::TextPosition{node:new_parent.children()[0],character_index:2};
        let fresh=accesskit::ActionRequest{data:Some(accesskit::ActionData::SetTextSelection(accesskit::TextSelection{anchor:position,focus:position})),..request};
        assert!(published.apply(&mut runtime,&fresh));assert_eq!(runtime.text_selection_utf16(),vec![2,2]);
    }

    #[test]
    fn keyed_binding_updates_keep_parent_ids_but_retire_text_positions() {
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } } }".into())]);
        let mut runtime=crate::Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
        let scene=runtime.native_scene_identity();let mut published=PublishedTree::default();let first=published.update(&runtime).unwrap();
        let (target,parent)=first.nodes.iter().find(|(_,node)|node.role()==Role::TextInput).unwrap();let old_run=parent.children()[0];
        let request=accesskit::ActionRequest{action:Action::SetValue,target_tree:TreeId::ROOT,target_node:*target,data:Some(accesskit::ActionData::Value("changed".into()))};
        assert!(published.apply(&mut runtime,&request));runtime.sync_ui_bindings_native().unwrap();assert_eq!(runtime.native_scene_identity(),scene);
        let next=published.update(&runtime).unwrap();let (new_target,new_parent)=next.nodes.iter().find(|(_,node)|node.role()==Role::TextInput).unwrap();assert_eq!(target,new_target);assert_ne!(old_run,new_parent.children()[0]);
        assert!(published.apply(&mut runtime,&request));
    }

    #[test]
    fn unchanged_publication_observes_text_disabled_and_source_identity_changes() {
        let files=serde_json::json!({"App.ui":"component App { Column { TextInput { key:'editor'; value:'seed'; } } }"}).to_string();
        let mut runtime=crate::Runtime::new();runtime.load_ui_files_json_native(&files,"App.ui","{}").unwrap();runtime.focus_control(0);
        let mut published=PublishedTree::default();assert!(published.update_if_changed(&runtime,1.).unwrap().is_some());
        assert!(published.update_if_changed(&runtime,1.).unwrap().is_none());
        runtime.text_preedit("marked");assert!(published.update_if_changed(&runtime,1.).unwrap().is_none());
        runtime.text_insert("X");assert!(published.update_if_changed(&runtime,1.).unwrap().is_some());
        runtime.set_binding_disabled(0,true).unwrap();assert!(published.update_if_changed(&runtime,1.).unwrap().is_some());
        runtime.load_ui_files_json_native(&files,"App.ui","{}").unwrap();runtime.focus_control(0);
        assert!(published.update_if_changed(&runtime,1.).unwrap().is_some());
        runtime.load_ui_files_json_native(&files,"App.ui","{}").unwrap();runtime.focus_control(0);
        assert!(published.update_if_changed(&runtime,1.).unwrap().is_some(),"identical snapshot in replacement scene must retire IDs");
        assert!(published.update_if_changed(&runtime,f64::NAN).is_err());assert!(published.update_if_changed(&runtime,0.).is_err());
    }

    #[test]
    fn slider_scroll_and_dialog_actions_route_and_publish_live_metadata() {
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state level: Number = 0.25; Column { Slider { key:'volume'; value <-> state.level; } } }".into())]);
        let mut runtime=crate::Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
        let semantic=runtime.accessibility_tree();
        let slider=semantic.nodes.iter().find(|n|n.key=="volume").unwrap().id;
        let update=tree_update(&semantic);
        let slider_node=&update.nodes.iter().find(|(key,_)|*key==id(slider)).unwrap().1;
        assert_eq!(slider_node.role(),Role::Slider);
        assert_eq!(slider_node.min_numeric_value(),Some(0.));
        assert_eq!(slider_node.max_numeric_value(),Some(1.));
        assert!(slider_node.supports_action(Action::Increment));
        assert!(slider_node.supports_action(Action::Decrement));
        assert!(slider_node.supports_action(Action::SetValue));
        assert!(apply_action(&mut runtime,id(slider),Action::Increment));
        assert!((runtime.range_value(0)-0.30).abs()<1e-5);
        assert!(apply_action(&mut runtime,id(slider),Action::Decrement));
        assert!((runtime.range_value(0)-0.25).abs()<1e-5);
        assert!(apply_action_data(&mut runtime,id(slider),Action::SetValue,Some(&accesskit::ActionData::NumericValue(0.75))));
        assert!((runtime.range_value(0)-0.75).abs()<1e-5);
        assert!(apply_action_data(&mut runtime,id(slider),Action::SetValue,Some(&accesskit::ActionData::Value("60%".into()))));
        assert!((runtime.range_value(0)-0.60).abs()<1e-5);
        assert!(!apply_action_data(&mut runtime,id(slider),Action::SetValue,Some(&accesskit::ActionData::NumericValue(-1.))));
        runtime.sync_ui_bindings_native().unwrap();
        let state:serde_json::Value=serde_json::from_str(&runtime.ui_state_json()).unwrap();
        assert!((state["level"].as_f64().unwrap()-0.60).abs()<1e-5);

        let layered="component Demo { Frame { width:200; height:80; clip:true; Scroll { Button { key:'base'; width:180; height:240; clicked -> actions.base(); } } Modal { key:'dialog'; x:10; y:10; width:180; height:60; dismissed -> actions.cancel(); Button { key:'ok'; width:80; height:28; clicked -> actions.ok(); } } } }";
        let visual="component Button { Rectangle { PointerArea { clicked -> events.clicked(); } } }";
        let mut modal_rt=crate::Runtime::from_sources(layered,visual).unwrap();
        let tree_open=modal_rt.accessibility_tree();
        let scroll_id=tree_open.nodes.iter().find(|n|n.role==A11yRole::ScrollView).unwrap().id;
        let dialog_id=tree_open.nodes.iter().find(|n|n.role==A11yRole::Dialog).unwrap().id;
        let update_open=tree_update(&tree_open);
        let dialog_node=&update_open.nodes.iter().find(|(key,_)|*key==id(dialog_id)).unwrap().1;
        assert_eq!(dialog_node.live(),Some(accesskit::Live::Polite));
        assert!(dialog_node.supports_action(Action::Collapse));
        assert!(!apply_action(&mut modal_rt,id(scroll_id),Action::ScrollDown),"active modal blocks background scroll");
        assert!(apply_action(&mut modal_rt,id(dialog_id),Action::Collapse));
        assert_eq!(modal_rt.active_modal(),-1);
        assert_eq!(modal_rt.dismiss_count(),1);
        assert!(apply_action(&mut modal_rt,id(scroll_id),Action::ScrollDown));
        assert!(modal_rt.scroll_offset()[1]>0.);
    }
}
