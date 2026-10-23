// The project a fresh Studio opens: a small search window with design states and a Rust contract.
export const sampleProject={
 'ui/SearchWindow.ui':`#[design('./SearchWindow.design.ui')]
component SearchWindow {
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
            clicked -> actions.search();
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
    Text { key: 'status'; text: '24 документа в библиотеке'; }

    state 'поиск' {
        TextInput { key: 'query'; value: 'Архитектура'; }
        Text { key: 'status'; text: 'Ищем…'; }
    }

    state 'нет результатов' {
        Button { key: 'searchButton'; disabled: true; }
        Text { key: 'status'; text: 'Ничего не найдено'; }
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
 'src/actions.rs':`// Контракт будущего Rust-backend.
// Этот файл редактируется, но не выполняется в web-preview.

pub struct SearchState {
    pub query: String,
    pub loading: bool,
    pub status: String,
}

pub fn search(state: &mut SearchState) {
    state.loading = true;
    // Подключите прикладной сервис поиска.
}
`,
 'README.md':'# Knowledge workspace\n\n.ui — разметка.\n.design.ui — состояния предпросмотра.\n\nОтладчик останавливается перед событием UI.\nПродолжить — применяет демонстрационный обработчик.\nRust-код требует будущей интеграции DAP.\n'
};
