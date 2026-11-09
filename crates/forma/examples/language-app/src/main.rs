use forma::binding::Property;
use std::rc::Rc;

#[derive(Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
}
pub struct TaskVm {
    pub tasks: Property<Vec<Task>>,
    pub show_info: Property<bool>,
    pub status: Property<String>,
}
include!(concat!(env!("OUT_DIR"), "/task_list.rs"));

fn control(form: &mut TaskList, suffix: &str) -> usize {
    let runtime = form.runtime().unwrap();
    (0..runtime.control_count())
        .find(|&i| runtime.control_key(i).ends_with(suffix))
        .unwrap()
}
fn main() -> Result<(), String> {
    let vm = Rc::new(TaskVm {
        tasks: Property::new(vec![
            Task {
                id: "design".into(),
                title: "Review the design".into(),
            },
            Task {
                id: "ship".into(),
                title: "Ship the update".into(),
            },
        ]),
        show_info: Property::new(true),
        status: Property::new("Ready".into()),
    });
    let mut form = TaskList::new(vm.clone())?;
    form.on_edited(|event| {
        event
            .context
            .status
            .set(format!("Edited {}: {}", event.id, event.title));
    });
    form.on_removed(|event| {
        let mut tasks = event.context.tasks.get();
        tasks.retain(|task| task.id != event.id);
        event.context.tasks.set(tasks);
    });
    form.on_toggle_info(|event| {
        event.context.show_info.set(!event.context.show_info.get());
    });

    let editor = control(&mut form, "editor");
    let editor_key = form.runtime()?.control_key(editor);
    form.update(|runtime| {
        runtime.focus_control(editor);
        runtime.text_key("a", false, true);
        runtime.text_insert("Design reviewed");
    })?;
    assert_eq!(vm.tasks.get()[0].title, "Design reviewed");
    assert_eq!(vm.status.get(), "Edited design: Design reviewed");

    let mut tasks = vm.tasks.get();
    tasks.reverse();
    vm.tasks.set(tasks);
    form.sync()?;
    let runtime = form.runtime()?;
    assert_eq!(
        runtime.control_key(runtime.focused_index() as usize),
        editor_key
    );

    let remove = control(&mut form, "remove");
    form.update(|runtime| runtime.activate_control(remove))?;
    assert_eq!(vm.tasks.get().len(), 1);
    assert_eq!(vm.tasks.get()[0].id, "design");
    let toggle = form.toggle().index();
    form.update(|runtime| runtime.activate_control(toggle))?;
    assert!(!vm.show_info.get());
    assert!(!form.runtime()?.pixels(560, 460, 1.).is_empty());
    println!(
        "Typed components, keyed edits/reorder/removal, events, Row/Grid and live expressions: OK"
    );
    Ok(())
}
