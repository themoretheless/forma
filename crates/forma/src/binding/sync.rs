//! Synchronizing form inputs and model bindings with the active runtime.

use super::*;

impl Form {
    pub fn sync(&mut self) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.wake.native.borrow_mut().drain();
        if self.is_dirty() {
            self.rebuild_document_scene()?;
        }
        self.push_bindings_to_runtime()?;
        self.clean_bindings();
        self.wake.pending.set(false);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = self.wake.native.borrow_mut().drain() > 0;
            if pending {
                self.wake.notify();
            }
        }
        Ok(())
    }

    /// Re-renders the document from its inputs and control overrides and replaces the runtime
    /// with the result. On any error the document keeps its previous state and scene.
    pub(super) fn rebuild_document_scene(&mut self) -> Result<(), String> {
        let Some(document) = &mut self.document else {
            return Ok(());
        };
        let has_control_overrides = self.controls.iter().any(|c| {
            document.nodes.contains_key(&c.index)
                && (!c.properties.borrow().is_empty() || !c.bindings.borrow().is_empty())
        });
        let saved_root = has_control_overrides.then(|| document.program.root.clone());
        let saved_states = document.program.states.clone();
        write_inputs_into_states(document);
        if has_control_overrides {
            write_control_overrides(document, &self.controls);
        }
        let rendered = document.program.render_scene_ir();
        if let Some(original_root) = saved_root {
            document.program.root = original_root;
        }
        let built = rendered.and_then(|ir| {
            check_event_arguments(document, &ir.controls)?;
            let runtime = Runtime::from_scene_ir(&ir)?;
            Ok((ir, runtime))
        });
        let (ir, mut next) = match built {
            Ok(built) => built,
            Err(error) => {
                document.program.states = saved_states;
                return Err(error);
            }
        };
        next.preserve_binding_interaction(&self.runtime);
        document.base_disabled = (0..next.control_count())
            .map(|i| next.control_disabled(i))
            .collect();
        let scene = &ir.controls;
        for control in &self.controls {
            let index = document
                .nodes
                .get(&control.index)
                .and_then(|node| scene.iter().position(|item| item.node == *node));
            control.resolved.set(index);
            if let Some(index) = index {
                control.base_disabled.set(next.control_disabled(index));
            }
        }
        document.scene = ir.controls;
        self.runtime = next;
        for (_, _, input) in &document.inputs {
            input.clean();
        }
        for control in &self.controls {
            for input in control.properties.borrow().values() {
                input.clean();
            }
        }
        Ok(())
    }

    /// Pushes binding values and the disabled state of every resolved control into the runtime.
    pub(super) fn push_bindings_to_runtime(&mut self) -> Result<(), String> {
        let sync_instances = self.sync_instances();
        for (control, index) in &sync_instances {
            if control.context_changed.get() {
                self.runtime.cancel_binding_interaction(*index);
            }
            for (target, binding) in control.bindings.borrow().iter() {
                if self.document.is_none() {
                    binding.sync(&mut self.runtime, *index, *target)?;
                } else if *target == Target::Value {
                    if let Binding::String(field, _) = binding.as_ref() {
                        if field.0.reset.get() {
                            self.runtime.set_binding_value(
                                *index,
                                &field.get().unwrap_or_default(),
                                true,
                            )?;
                        }
                    }
                }
            }
            let mut disabled = self.disabled(control);
            if let Some(doc) = &self.document {
                if (control.present)() && !control.bindings.borrow().contains_key(&Target::Disabled)
                {
                    disabled = doc.base_disabled.get(*index).copied().unwrap_or(true);
                }
            }
            self.runtime.set_binding_disabled(*index, disabled)?;
        }
        Ok(())
    }

    /// Marks every binding as delivered.
    pub(super) fn clean_bindings(&self) {
        for control in &self.controls {
            control.context_changed.set(false);
            for binding in control.bindings.borrow().values() {
                match binding.as_ref() {
                    Binding::String(f, _) => {
                        f.0.dirty.set(false);
                        f.0.reset.set(false);
                    }
                    Binding::Number(f, _) => f.clean(),
                    Binding::Group(g) => g.clean(),
                    Binding::Bool(f) => {
                        f.0.dirty.set(false);
                        f.0.reset.set(false);
                    }
                }
            }
        }
    }

    pub(super) fn sync_instances(&self) -> Vec<(Rc<ControlState>, usize)> {
        if let Some(document) = &self.document {
            document
                .scene
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let control_index = document.node_controls.get(&item.node)?;
                    let control = self
                        .controls
                        .get(*control_index)
                        .filter(|c| c.index == *control_index)
                        .or_else(|| self.controls.iter().find(|c| c.index == *control_index))?;
                    Some((control.clone(), index))
                })
                .collect()
        } else {
            self.controls.iter().map(|c| (c.clone(), c.index)).collect()
        }
    }
}
