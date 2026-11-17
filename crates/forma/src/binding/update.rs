//! What one `Form::update` observed and the writes it queues.

use super::*;

/// What a control showed and was bound to before an update ran, each binding with the epoch
/// its field had then.
pub(super) struct ControlSnapshot {
    pub(super) text: String,
    pub(super) range: f32,
    pub(super) clicks: u32,
    pub(super) submissions: u32,
    pub(super) context_epoch: u64,
    pub(super) bindings: Vec<(Target, Rc<Binding>, u64)>,
    pub(super) collections: Vec<(CollectionBinding, u64)>,
}

impl ControlSnapshot {
    pub(super) fn take(runtime: &Runtime, control: &ControlState, index: usize) -> Self {
        Self {
            text: runtime.text_value(index),
            range: runtime.range_value(index),
            clicks: runtime.control_clicks(index),
            submissions: runtime.control_submissions(index),
            context_epoch: control.context_epoch.get(),
            bindings: control
                .bindings
                .borrow()
                .iter()
                .map(|(target, binding)| {
                    (
                        *target,
                        binding.clone(),
                        match binding.as_ref() {
                            Binding::String(f, _) => f.0.epoch.get(),
                            Binding::Number(f, _) => f.epoch(),
                            Binding::Bool(f) => f.0.epoch.get(),
                            Binding::Group(g) => g.epoch(),
                        },
                    )
                })
                .collect(),
            collections: control
                .collection_bindings
                .borrow()
                .iter()
                .map(|b| (b.clone(), b.field.epoch()))
                .collect(),
        }
    }
}

/// What the input did to one control.
pub(super) struct ControlChange {
    pub(super) text: String,
    pub(super) next_text: String,
    pub(super) range: f32,
    pub(super) next_range: f32,
    /// `next_range` on the slider's own scale (`min` to `max`): what the model is given.
    pub(super) next_scaled: f32,
    pub(super) clicks: u32,
}

impl ControlChange {
    pub(super) fn text_changed(&self) -> bool {
        self.text != self.next_text
    }
    pub(super) fn range_changed(&self) -> bool {
        self.range.is_finite() && self.range != self.next_range
    }
    pub(super) fn value_changed(&self) -> bool {
        self.text_changed() || self.range_changed()
    }
}

/// A handler to run once the batch's writes are committed.
pub(super) struct QueuedEvent {
    pub(super) control: Rc<ControlState>,
    pub(super) key: String,
    pub(super) context_epoch: u64,
    pub(super) event: &'static str,
    pub(super) handler: Handler,
}

/// The writes an update produced, each with the epoch its field had before the input ran. A
/// field whose epoch moved since then was rebound and is left alone.
#[derive(Default)]
pub(super) struct PendingWrites {
    pub(super) strings: Vec<(Field<String>, String, u64)>,
    pub(super) numbers: Vec<(Rc<dyn NumericInput>, f64, u64)>,
    pub(super) bools: Vec<(Field<bool>, bool, u64)>,
    pub(super) groups: Vec<(Rc<dyn GroupInput>, u64)>,
    pub(super) collections: Vec<CollectionItemWrite>,
}

/// A value for one item of a bound collection, addressed by its loop indices.
pub(super) struct CollectionItemWrite {
    pub(super) field: Rc<dyn CollectionWrite>,
    pub(super) indices: Vec<usize>,
    pub(super) value: crate::form_document::Value,
    pub(super) epoch: u64,
}

impl PendingWrites {
    /// Queues the writes of a control's two-way bindings. Returns whether a toggle fired.
    pub(super) fn collect_bindings(
        &mut self,
        control: &ControlState,
        bindings: Vec<(Target, Rc<Binding>, u64)>,
        change: &ControlChange,
    ) -> Result<bool, String> {
        let mut toggled = false;
        for (target, binding, epoch) in bindings {
            if !control
                .bindings
                .borrow()
                .get(&target)
                .is_some_and(|current| Rc::ptr_eq(current, &binding))
            {
                continue;
            }
            match binding.as_ref() {
                Binding::String(field, true)
                    if target == Target::Value && change.text_changed() =>
                {
                    self.strings
                        .push((field.clone(), change.next_text.clone(), epoch))
                }
                Binding::Number(field, true)
                    if target == Target::Value && change.range_changed() =>
                {
                    self.numbers
                        .push((field.clone(), change.next_scaled as f64, epoch))
                }
                Binding::Number(field, true)
                    if target == Target::Value && change.text_changed() =>
                {
                    let value = change
                        .next_text
                        .parse::<f64>()
                        .map_err(|_| "Numeric input expects a finite number")?;
                    self.numbers.push((field.clone(), value, epoch));
                }
                Binding::Bool(field)
                    if (target == Target::Checked && change.clicks % 2 == 1)
                        || (target == Target::Selected && change.clicks > 0) =>
                {
                    self.bools.push((
                        field.clone(),
                        if target == Target::Selected {
                            true
                        } else {
                            !field.get().unwrap_or(false)
                        },
                        epoch,
                    ));
                    toggled = true;
                }
                Binding::Group(group)
                    if target == Target::Group && change.clicks > 0 && !group.chosen() =>
                {
                    self.groups.push((group.clone(), epoch));
                    toggled = true;
                }
                _ => {}
            }
        }
        Ok(toggled)
    }

    /// Queues the writes of a control's bindings into collection items, addressed through the
    /// loop indices in `locals`. Returns whether a toggle fired.
    pub(super) fn collect_collections(
        &mut self,
        collections: Vec<(CollectionBinding, u64)>,
        locals: &crate::form_document::Properties,
        change: &ControlChange,
    ) -> Result<bool, String> {
        use crate::form_document::Value;
        let mut toggled = false;
        for (binding, epoch) in collections {
            let item_indices = binding
                .indices
                .iter()
                .map(|key| {
                    locals
                        .get(key)
                        .and_then(|v| v.number().ok())
                        .map(|v| v as usize)
                })
                .collect::<Option<Vec<_>>>();
            let Some(item_indices) = item_indices else {
                continue;
            };
            let value = match binding.target.as_str() {
                "value" if change.range_changed() => Some(Value::Number(change.next_scaled as f64)),
                "value" if change.text_changed() => Some(Value::Text(change.next_text.clone())),
                "checked" if change.clicks % 2 == 1 => {
                    toggled = true;
                    Some(Value::Bool(
                        !Value::Expr(binding.value_path.clone())
                            .evaluate(&Default::default(), locals)?
                            .boolean()?,
                    ))
                }
                "selected" if change.clicks > 0 => {
                    toggled = true;
                    Some(Value::Bool(true))
                }
                _ => None,
            };
            if let Some(value) = value {
                self.collections.push(CollectionItemWrite {
                    field: binding.field,
                    indices: item_indices,
                    value,
                    epoch,
                });
            }
        }
        Ok(toggled)
    }

    /// Prepares every write and returns the commits. Nothing observable changes until they
    /// run, so a write that fails validation leaves the whole batch unapplied.
    pub(super) fn stage(self) -> Result<Vec<Box<dyn FnOnce()>>, String> {
        let mut commits: Vec<Box<dyn FnOnce()>> = Vec::with_capacity(
            self.strings.len() + self.numbers.len() + self.bools.len() + self.collections.len(),
        );
        for (field, value, epoch) in self.strings {
            commits.push(Box::new(move || {
                if field.0.epoch.get() == epoch {
                    field.set(value);
                }
            }));
        }
        for (field, value, epoch) in self.numbers {
            if field.epoch() == epoch {
                let commit = field.prepare_number(value)?;
                commits.push(Box::new(move || {
                    if field.epoch() == epoch {
                        commit();
                    }
                }));
            }
        }
        for (field, value, epoch) in self.bools {
            commits.push(Box::new(move || {
                if field.0.epoch.get() == epoch {
                    field.set(value);
                }
            }));
        }
        for (group, epoch) in self.groups {
            if group.epoch() == epoch {
                let commit = group.prepare_choose()?;
                commits.push(Box::new(move || {
                    if group.epoch() == epoch {
                        commit();
                    }
                }));
            }
        }
        for CollectionItemWrite {
            field,
            indices,
            value,
            epoch,
        } in self.collections
        {
            if field.epoch() == epoch {
                if let Some(commit) = field.prepare_write(&indices, &value)? {
                    commits.push(Box::new(move || {
                        if field.epoch() == epoch {
                            commit();
                        }
                    }));
                }
            }
        }
        Ok(commits)
    }
}
