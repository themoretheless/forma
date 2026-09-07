use forma::binding::{Context, Property};
use std::rc::Rc;

pub struct AddressVm {
    pub city: Property<String>,
}
pub struct CustomerVm {
    pub name: Property<String>,
    pub email: Property<String>,
    pub busy: Property<bool>,
    pub status: Property<String>,
    pub address: Context<AddressVm>,
}
impl CustomerVm {
    pub fn new(name: &str, city: &str) -> Rc<Self> {
        Rc::new(Self {
            name: Property::new(name.into()),
            email: Property::new(format!("{name}@example.com")),
            busy: Property::new(false),
            status: Property::new("Готово".into()),
            address: Context::new(Rc::new(AddressVm {
                city: Property::new(city.into()),
            })),
        })
    }
    pub fn save(&self) {
        self.status.set(format!("Сохранено: {}", self.name.get()));
    }
}

include!(concat!(env!("OUT_DIR"), "/customer_form.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_forms_share_model_but_not_focus_and_propagate_in_both_directions() {
        let model = CustomerVm::new("Анна", "Тбилиси");
        let mut a = CustomerForm::new(model.clone()).unwrap();
        let mut b = CustomerForm::new(model.clone()).unwrap();
        a.update(|r| {
            r.focus_control(0);
            r.text_key("a", false, true);
            r.text_insert("Мария🙂");
        })
        .unwrap();
        assert_eq!(model.name.get(), "Мария🙂");
        assert_eq!(b.runtime().unwrap().text_value(0), "Мария🙂");
        assert_eq!(a.runtime().unwrap().focused_index(), 0);
        assert_eq!(b.runtime().unwrap().focused_index(), -1);
        // Model echo must not destroy the originating editor's undo history.
        a.update(|r| r.text_key("z", false, true)).unwrap();
        assert_eq!(model.name.get(), "Анна");
        model.name.set("Из кода".into());
        assert_eq!(a.runtime().unwrap().text_value(0), "Из кода");
        assert_eq!(b.runtime().unwrap().text_value(0), "Из кода");
        let independent = CustomerVm::new("Другой", "Ереван");
        b.set_context(independent.clone()).unwrap();
        model.name.set("Первый".into());
        assert_eq!(b.runtime().unwrap().text_value(0), "Другой");
    }

    #[test]
    fn local_context_and_explicit_field_rebinding_are_independent() {
        let root = CustomerVm::new("Root", "A");
        let local = CustomerVm::new("Local", "B");
        let next = CustomerVm::new("Next", "C");
        let mut form = CustomerForm::new(root.clone()).unwrap();
        form.name_input().set_context(local.clone());
        form.set_context(next.clone()).unwrap();
        assert_eq!(form.runtime().unwrap().text_value(0), "Local");
        form.name_input().inherit_context();
        assert_eq!(form.runtime().unwrap().text_value(0), "Next");
        form.name_input().bind_value(root.email.clone());
        form.set_context(local.clone()).unwrap();
        assert_eq!(form.runtime().unwrap().text_value(0), "Root@example.com");
        form.update(|r| {
            r.focus_control(0);
            r.text_key("a", false, true);
            r.text_insert("changed@email");
        })
        .unwrap();
        assert_eq!(root.email.get(), "changed@email");
        assert_eq!(local.name.get(), "Local");
        form.name_input()
            .bind_value_path(|vm| vm.name.clone(), true);
        assert_eq!(form.runtime().unwrap().text_value(0), "Local");
    }

    #[test]
    fn replacing_nested_context_detaches_old_source_and_empty_context_disables_input() {
        let model = CustomerVm::new("Name", "Old");
        let old = model.address.get().unwrap();
        let mut form = CustomerForm::new(model.clone()).unwrap();
        model.address.set(Rc::new(AddressVm {
            city: Property::new("New".into()),
        }));
        assert_eq!(form.runtime().unwrap().text_value(1), "New");
        old.city.set("Obsolete".into());
        assert!(!form.is_dirty());
        model.address.clear();
        assert_eq!(form.runtime().unwrap().text_value(1), "");
        assert!(form.runtime().unwrap().control_disabled(1));
        assert!(!form.update(|r| r.focus_control(1)).unwrap());
        model.address.set(old);
        assert_eq!(form.runtime().unwrap().text_value(1), "Obsolete");
        assert!(!form.runtime().unwrap().control_disabled(1));
    }

    #[test]
    fn generated_event_uses_current_context_and_updates_the_renderer() {
        let a = CustomerVm::new("A", "City");
        let b = CustomerVm::new("B", "City");
        let mut form = CustomerForm::new(a.clone()).unwrap();
        form.on_save_requested(|event| event.context.save());
        form.set_context(b.clone()).unwrap();
        form.update(|r| r.activate_control(2)).unwrap();
        assert_eq!(a.status.get(), "Готово");
        assert_eq!(b.status.get(), "Сохранено: B");
        assert_eq!(form.runtime().unwrap().control_label(3), "Сохранено: B");
        b.busy.set(true);
        b.status.set("Busy".into());
        form.update(|r| r.activate_control(2)).unwrap();
        assert_eq!(b.status.get(), "Busy");
        form.clear_context().unwrap();
        assert!(form.runtime().unwrap().control_disabled(2));
    }

    #[test]
    fn replacement_with_equal_text_resets_undo_and_cancels_an_old_gesture() {
        let a = CustomerVm::new("A", "City");
        let b = CustomerVm::new("AB", "City");
        let mut form = CustomerForm::new(a.clone()).unwrap();
        form.update(|r| {
            r.focus_control(0);
            r.text_key("End", false, false);
            r.text_insert("B");
        })
        .unwrap();
        form.set_context(b.clone()).unwrap();
        assert_eq!(form.runtime().unwrap().focused_index(), 0);
        form.update(|r| r.text_key("z", false, true)).unwrap();
        assert_eq!(form.runtime().unwrap().text_value(0), "AB");
        form.on_save_requested(|event| event.context.save());
        let bounds = form.runtime().unwrap().control_bounds(2);
        form.update(|r| r.pointer(bounds[0] + 10., bounds[1] + 10., 1))
            .unwrap();
        form.set_context(a.clone()).unwrap();
        form.update(|r| r.pointer(bounds[0] + 10., bounds[1] + 10., 2))
            .unwrap();
        assert_eq!(a.status.get(), "Готово");
    }

    #[test]
    fn model_changes_schedule_one_redraw_and_dropped_forms_release_handlers() {
        use std::cell::Cell;
        let model = CustomerVm::new("Name", "City");
        let mut form = CustomerForm::new(model.clone()).unwrap();
        let count = Rc::new(Cell::new(0));
        let calls = count.clone();
        form.on_dirty(move || calls.set(calls.get() + 1));
        model.name.set("First".into());
        model.status.set("Second".into());
        assert_eq!(count.get(), 1);
        assert!(form.is_dirty());
        form.sync().unwrap();
        model.name.set("Third".into());
        assert_eq!(count.get(), 2);
        let capture = Rc::new(());
        let weak = Rc::downgrade(&capture);
        form.on_save_requested(move |_| {
            let _ = &capture;
        });
        let detached = form.name_input();
        drop(form);
        assert!(weak.upgrade().is_none());
        model.name.set("After drop".into());
        assert_eq!(count.get(), 2);
        drop(detached);
    }

    #[test]
    fn bound_text_invalidates_rendered_pixels_without_reloading_layout() {
        let model = CustomerVm::new("Name", "City");
        let mut form = CustomerForm::new(model.clone()).unwrap();
        let before = form.runtime().unwrap().pixels(360, 290, 1.);
        let layout = form.runtime().unwrap().layout_revision();
        model.status.set("Другая надпись".into());
        let after = form.runtime().unwrap().pixels(360, 290, 1.);
        assert_ne!(before, after);
        assert_eq!(form.runtime().unwrap().layout_revision(), layout);
    }

    #[test]
    fn context_replacement_during_input_does_not_write_old_text_to_new_record() {
        let old = CustomerVm::new("Old", "City");
        let next = CustomerVm::new("Next", "City");
        let mut form = CustomerForm::new(old.clone()).unwrap();
        let context = form.context();
        form.update(|r| {
            r.focus_control(0);
            r.text_key("a", false, true);
            r.text_insert("Stale input");
            context.set(next.clone());
        })
        .unwrap();
        assert_eq!(next.name.get(), "Next");
        assert_eq!(form.runtime().unwrap().text_value(0), "Next");
    }
}
