// The project a fresh Studio opens: design states in Studio, real state and commands in Rust.
export const sampleProject={
 'Cargo.toml':`[package]
name = "forma-knowledge-workspace"
version = "0.1.0"
edition = "2021"
publish = false

[features]
default = []
native = ["forma/native"]

[dependencies]
forma = { path = "../vector-ui" }
`,
 'ui/SearchWindow.ui':`#[design('./SearchWindow.design.ui')]
component SearchWindow {
    contextType: 'crate::SearchVm';

    Frame {
        padding: 32;
        gap: 16;

        Text {
            text: 'Библиотека знаний';
            font.size: 26;
            color: #e8edf7;
        }

        Text {
            text: 'Найдите ответ в ваших документах';
            color: #98a4ba;
        }

        TextInput {
            key: 'query';
            value <-> state.query;
            placeholder: 'Что найти?';
            padding: 12;
        }

        Button {
            key: 'searchButton';
            style: primary;
            text: 'Найти документы';
            disabled: state.loading;
            clicked -> state.search();
        }

        Button {
            key: 'resetButton';
            text: 'Сбросить запрос';
            disabled: state.loading;
            clicked -> state.reset();
        }

        Text {
            key: 'status';
            text: state.status;
            color: '#98a4ba';
        }
    }
}`,
 'ui/SearchWindow.design.ui':`design SearchWindow {
    TextInput { key: 'query'; value: ''; }
    Button { key: 'searchButton'; disabled: false; }
    Button { key: 'resetButton'; disabled: false; }
    Text { key: 'status'; text: '3 документа в библиотеке'; }

    state 'поиск' {
        TextInput { key: 'query'; value: 'Архитектура'; }
        Text { key: 'status'; text: 'Ищем…'; }
    }

    state 'нет результатов' {
        Button { key: 'searchButton'; disabled: true; }
        Text { key: 'status'; text: 'Ничего не найдено'; }
    }
}`,
 'ui/TwoButtons.ui':`component TwoButtons {
    Frame {
        width: 380;
        height: 240;
        padding: 30;
        gap: 20;
        background: #181e2a;
        radius: 16;
        clip: true;

        Button {
            key: 'first';
            width: 320;
            height: 70;
            text: 'Первая кнопка';
            clicked -> actions.first();
        }

        Button {
            key: 'second';
            width: 320;
            height: 70;
            text: 'Вторая кнопка';
            background: #aad9bd;
            hoverBackground: #c7eed7;
            pressedBackground: #78b493;
            clicked -> actions.second();
        }
    }
}`,
 'components/Button.ui':`component Button {
    width: 456;
    height: 44;
    text: 'Кнопка';
    style: 'default';
    fontSize: 15;
    color: #14213b;
    radius: 8;
    disabled: false;
    background: #8fa8ff;
    hoverBackground: #a8baff;
    pressedBackground: #6a83da;
    disabledBackground: #596273;
    borderWidth: 1;
    borderColor: #bed0ff;
    focusBorderColor: #ffffff;
    transitionDuration: 140ms;

    Rectangle {
        radius: props.radius;

        background: Brush {
            color: props.background;
            hover: props.hoverBackground;
            pressed: props.pressedBackground;
            disabled: props.disabledBackground;
            transition: props.transitionDuration;
        };

        Border {
            width: props.borderWidth;
            background: Brush {
                color: props.borderColor;
                focus: props.focusBorderColor;
                transition: props.transitionDuration;
            };
        }

        Text {
            text: props.text;
            color: props.color;
            fontSize: props.fontSize;
        }

        PointerArea {
            clicked -> events.clicked();
        }
    }
}`,
 'components/TextInput.ui':`component TextInput {
    width: 456;
    height: 42;
    value: '';
    placeholder: '';
    padding: 12;
    multiline: false;
    text: props.value;
    fontSize: 15;
    color: #e8edf7;
    placeholderColor: #76839b;
    background: #141a25;
    hoverBackground: #18202e;
    pressedBackground: #141a25;
    borderWidth: 1;
    borderColor: #30394a;
    focusBorderColor: #8fa8ff;
    radius: 8;
    transitionDuration: 140ms;

    Rectangle {
        radius: props.radius;

        background: Brush {
            color: props.background;
            hover: props.hoverBackground;
            pressed: props.pressedBackground;
            transition: props.transitionDuration;
        };

        Border {
            width: props.borderWidth;
            background: Brush {
                color: props.borderColor;
                focus: props.focusBorderColor;
                transition: props.transitionDuration;
            };
        }

        Frame {
            padding: props.padding;
            TextInput {
                forward props { value, placeholder, multiline, fontSize, color, placeholderColor };
            }
        }

        PointerArea {
            clicked -> events.clicked();
        }
    }
}`,
 'src/app.rs':`use crate::forma_generated::SearchWindow;
use forma::binding::Property;
use std::rc::Rc;

const DOCUMENTS: &[&str] = &[
    "Архитектура Forma: разметка, ViewModel и события",
    "Rust Property: наблюдаемое состояние приложения",
    "Дизайн-состояния и сценарии предпросмотра",
];

// Поля Property связываются с .ui двусторонне или в одну сторону.
pub struct SearchVm {
    pub query: Property<String>,
    pub loading: Property<bool>,
    pub status: Property<String>,
}

impl SearchVm {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            query: Property::new(String::new()),
            loading: Property::new(false),
            status: Property::new("3 документа в библиотеке".into()),
        })
    }

    // Кнопка вызывает этот метод через clicked -> state.search().
    pub fn search(&self) {
        self.loading.set(true);
        let query = self.query.get().trim().to_lowercase();
        if query.is_empty() {
            self.status.set("Введите запрос".into());
        } else {
            let count = DOCUMENTS
                .iter()
                .filter(|document| document.to_lowercase().contains(&query))
                .count();
            self.status.set(if count == 0 {
                "Ничего не найдено".into()
            } else {
                format!("Найдено документов: {count}")
            });
        }
        self.loading.set(false);
    }

    // Вторая кнопка вызывает этот метод через clicked -> state.reset().
    pub fn reset(&self) {
        self.query.set(String::new());
        self.loading.set(false);
        self.status.set("3 документа в библиотеке".into());
    }
}

pub fn create_form() -> Result<SearchWindow, String> {
    SearchWindow::new(SearchVm::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_updates_the_live_view_model() {
        let model = SearchVm::new();
        model.query.set("архитектура".into());
        model.search();
        assert_eq!(model.status.get(), "Найдено документов: 1");

        model.query.set("  ".into());
        model.search();
        assert_eq!(model.status.get(), "Введите запрос");

        model.reset();
        assert_eq!(model.query.get(), "");
        assert_eq!(model.status.get(), "3 документа в библиотеке");
    }

    #[test]
    fn search_button_dispatches_through_the_generated_form() {
        let model = SearchVm::new();
        let mut form = SearchWindow::new(model.clone()).unwrap();
        model.query.set("архитектура".into());
        // Control indices address the expanded native scene, including component internals.
        form.update(|runtime| runtime.activate_control(3)).unwrap();
        assert_eq!(model.status.get(), "Найдено документов: 1");
        form.update(|runtime| runtime.activate_control(4)).unwrap();
        assert_eq!(model.status.get(), "3 документа в библиотеке");
    }
}
`,
 'src/main.rs':`mod app;
mod forma_generated;

pub use app::*;

#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    use forma::native_window::{run, NativeWindowConfig};

    let mut form = app::create_form()?;
    let (width, height) = {
        let runtime = form.runtime()?;
        (runtime.render_width().max(1.0), runtime.render_height().max(1.0))
    };
    run(form, NativeWindowConfig {
        title: "Библиотека знаний".into(),
        width,
        height,
        ..Default::default()
    })
}

#[cfg(not(feature = "native"))]
fn main() -> Result<(), String> {
    let mut form = app::create_form()?;
    let pixels = form.runtime()?.pixels(520, 320, 1.0);
    println!("Headless render: {} байт RGBA", pixels.len());
    println!("Статус ViewModel: {}", form.context().get().unwrap().status.get());
    Ok(())
}
`,
 'README.md':'# Библиотека знаний\n\nСтартовый проект показывает настоящую связку `.ui ↔ Rust ViewModel`:\n\n- `ui/SearchWindow.ui` привязывает ввод к `SearchVm.query`, статус — к `SearchVm.status`.\n- `clicked -> state.search()` вызывает метод `SearchVm::search` из `src/app.rs`.\n- `ui/SearchWindow.design.ui` задаёт только состояния дизайнера и не заменяет Rust-состояние приложения.\n- `src/main.rs` запускает окно с feature `native`; без неё выполняется headless render.\n- `src/forma_generated.rs` создаётся автоматически при запуске и экспорте.\n\nВ Studio нажмите «▶ Приложение» и выполните поиск. В экспортированном проекте: `cargo run --features native`; тесты ViewModel и headless render: `cargo test`.\n\nWeb/HTML-предпросмотр показывает разметку и дизайн-состояния, но не исполняет Rust-код.\n'
};
