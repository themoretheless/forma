//! Values a form reads from and writes to a model.

use super::*;

pub(super) trait Input {
    fn present(&self) -> bool;
    fn dirty(&self) -> bool;
    fn value(&self) -> crate::form_document::Value;
    fn clean(&self);
}

impl<T: ModelValue> Input for Field<T> {
    fn present(&self) -> bool {
        self.get().is_some()
    }
    fn dirty(&self) -> bool {
        self.0.dirty.get()
    }
    fn value(&self) -> crate::form_document::Value {
        self.get().unwrap_or_default().to_value()
    }
    fn clean(&self) {
        self.0.dirty.set(false);
    }
}

pub(super) trait NumericInput {
    fn present(&self) -> bool;
    fn dirty(&self) -> bool;
    fn epoch(&self) -> u64;
    fn number(&self) -> f64;
    fn clean(&self);
    fn prepare_number(&self, value: f64) -> Result<Box<dyn FnOnce()>, String>;
}

impl<T: ModelNumber> NumericInput for Field<T> {
    fn present(&self) -> bool {
        self.get().is_some()
    }
    fn dirty(&self) -> bool {
        self.0.dirty.get()
    }
    fn epoch(&self) -> u64 {
        self.0.epoch.get()
    }
    fn number(&self) -> f64 {
        self.get()
            .unwrap_or_default()
            .to_value()
            .number()
            .unwrap_or_default()
    }
    fn clean(&self) {
        self.0.dirty.set(false);
        self.0.reset.set(false);
    }
    fn prepare_number(&self, value: f64) -> Result<Box<dyn FnOnce()>, String> {
        let converted = T::from_value(&crate::form_document::Value::Number(value))?;
        let field = self.clone();
        Ok(Box::new(move || field.set(converted)))
    }
}

pub(super) struct ProjectedInput<T> {
    pub(super) field: Field<T>,
    pub(super) project: Rc<dyn Fn(&T) -> crate::form_document::Value>,
}

impl<T: Clone + PartialEq + Default + 'static> Input for ProjectedInput<T> {
    fn present(&self) -> bool {
        self.field.get().is_some()
    }
    fn dirty(&self) -> bool {
        self.field.0.dirty.get()
    }
    fn value(&self) -> crate::form_document::Value {
        (self.project)(&self.field.get().unwrap_or_default())
    }
    fn clean(&self) {
        self.field.0.dirty.set(false);
    }
}

pub(super) trait CollectionWrite {
    fn epoch(&self) -> u64;
    fn prepare_write(
        &self,
        indices: &[usize],
        value: &crate::form_document::Value,
    ) -> Result<Option<Box<dyn FnOnce()>>, String>;
}

pub(super) type CollectionItemSetter<T> =
    Rc<dyn Fn(&mut T, &[usize], &crate::form_document::Value) -> Result<(), String>>;

pub(super) struct CollectionField<T> {
    pub(super) field: Field<Vec<T>>,
    pub(super) set: CollectionItemSetter<T>,
}

impl<T: Clone + PartialEq + 'static> CollectionWrite for CollectionField<T> {
    fn epoch(&self) -> u64 {
        self.field.0.revision.get()
    }
    fn prepare_write(
        &self,
        indices: &[usize],
        value: &crate::form_document::Value,
    ) -> Result<Option<Box<dyn FnOnce()>>, String> {
        let Some(mut items) = self.field.get() else {
            return Ok(None);
        };
        let Some(index) = indices.first() else {
            return Err("Collection binding requires an item index".into());
        };
        let Some(item) = items.get_mut(*index) else {
            return Err("Repeated control no longer has a collection item".into());
        };
        (self.set)(item, indices, value)?;
        let field = self.field.clone();
        Ok(Some(Box::new(move || field.set(items))))
    }
}
