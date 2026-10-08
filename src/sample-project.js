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
