use forma_binding_example::{CustomerForm, CustomerVm};

#[cfg(feature = "native")]
mod window;

#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    window::run()
}

#[cfg(not(feature = "native"))]
fn main() -> Result<(), String> {
    let customer = CustomerVm::new("Анна", "Тбилиси");
    let mut form = CustomerForm::new(customer.clone())?;
    form.on_save_requested(|event| event.context.save());
    form.update(|runtime| {
        runtime.focus_control(0);
        runtime.text_key("a", false, true);
        runtime.text_insert("Мария");
        runtime.activate_control(2);
    })?;
    println!("{}", customer.status.get());
    let pixels = form.runtime()?.pixels(360, 290, 1.);
    println!(
        "Rendered {} RGBA bytes through the shared Rust renderer",
        pixels.len()
    );
    Ok(())
}
