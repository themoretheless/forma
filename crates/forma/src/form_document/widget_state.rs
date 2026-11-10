//! What binding synchronization needs to read from the live widgets.
//!
//! The document layer writes two-way bindings back into state from whatever hosts the widgets.
//! Taking this trait instead of the concrete runtime keeps the dependency pointing downward: the
//! runtime knows the document, the document does not know the runtime.

pub trait WidgetState {
    /// Number of live controls; indices below are in `0..control_count()`.
    fn control_count(&self) -> usize;
    /// The stable key of a control, used to match it after the scene was rebuilt.
    fn control_key(&self, index: usize) -> String;
    /// Whether the user changed this control's value since the last acknowledgement.
    fn binding_widget_changed(&self, index: usize) -> bool;
    /// Whether this control has a text editor.
    fn control_editable(&self, index: usize) -> bool;
    /// The current text of an editable control.
    fn text_value(&self, index: usize) -> String;
    /// The current position of a range control, from 0 to 1.
    fn range_value(&self, index: usize) -> f32;
    /// Marks every current widget value as seen, so the next sync reports only new changes.
    fn acknowledge_binding_widgets(&self);
}
