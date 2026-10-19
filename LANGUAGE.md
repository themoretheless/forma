# Язык Forma

Исходники языка живут в репозитории
[tokenizer](https://github.com/themoretheless/tokenizer): полный Rust frontend —
`crates/tokenizer-forma`, WASM-адаптеры Studio — `packages/forma`. Разметка,
выражения, семантика компонентов и Lezer-грамматика изменяются там.
Парсер, выражения, контракты, design-состояния и раскрытие if/for исполняются
в Rust; JavaScript соединяет их с редактором и моделями Studio.

Forma берёт `themoretheless-tokenizer-forma` из ветки `release`, точный коммит
закреплён в `vector-ui/Cargo.lock`. JS-пакет `@themoretheless/tokenizer-forma`
подключается из checkout того же коммита в соседнем каталоге `../tokenizer`:
`npm run tokenizer -- checkout install build` клонирует его (если каталога нет),
проверяет совпадение с lock, ставит зависимости пакета и собирает WASM языка.
Уже существующий checkout не трогается: `npm run tokenizer -- check` падает,
если `crates/tokenizer-forma` или `packages/forma` в нём отличаются от
закреплённой ревизии. Переход на новый релиз tokenizer:
`cargo update --manifest-path vector-ui/Cargo.toml -p themoretheless-tokenizer-forma`
(и так же для `vector-ui/examples/*/Cargo.toml`).
Перегенерация редакторского парсера: `npm run generate` в `tokenizer/packages/forma`.

Один и тот же `.ui` разбирается компилятором Studio и генератором Rust-форм.
Studio вычисляет выражения по данным предпросмотра. Сгенерированная форма
подписывается на `Property<T>` и вычисляет их в Rust при изменении модели.
Интерактивный векторный предпросмотр записывает изменения привязанных полей
в своё состояние; пользовательская Rust ViewModel запускается кнопкой приложения.

## Контракт компонента

```text
component Notice : Button {
    enum Tone { Accent, Danger }
    required prop title: String;
    prop tone: Tone = Tone.Accent;
    prop subtitle: String?;

    text: props.title;
}
```

`required prop` требует значение у экземпляра или в наследнике. Резервное
значение движка не удовлетворяет обязательному параметру. Обычный `prop`
получает значение после `=`; необязательный `T?` без значения получает `null`.
Старые объявления вида `text: '';` сохраняются. Строгий публичный контракт
задаётся через `prop`.

Типы: `String`, `Bool` (`Boolean`), `Number` (`Float`), `Int`, `Color`,
`Length`, `Angle`, `Duration`, `Asset`, коллекции `List<T>` (`T[]`), словари
`Record<K, V>` (`Map<T>`), объединения `A | B`, объявленные `enum` и `struct`,
а также необязательные варианты `T?`.
Числа должны быть конечными. `Int` проверяется как целое число, `Color` —
как hex-цвет, `Duration` — как значение в `ms`, `Angle` — как градусы/радианы/обороты
(`deg`, `rad`, `turn`) или число. `Asset` обозначает строку пути;
проверка конкретного ресурса происходит при его использовании.

Для составных моделей прямо в компоненте можно объявлять `struct`:

```text
struct User {
    name: String;
    age: Int = 18;
}
prop author: User = { name: 'Ada', age: 28 };
prop tags: List<String> = ['ui', 'forma'];
prop id: String | Int = 42;
prop meta: Record<String, Number> = { score: 100 };
```

Валидация свойств задаётся атрибутами `#[range(min, max)]` и `#[pattern('regex')]`:

```text
#[range(0, 100)]
prop progress: Int = 50;

#[pattern('^[a-z0-9_-]+$')]
prop slug: String = 'forma-ui';
```

Внутри выражений доступно вычисляемое свойство `isValid` (`Bool`), показывающее,
проходят ли все ограничения `#[range]` и `#[pattern]` текущие значения свойств.

```text
Notice {
    title: 'Не удалось сохранить';
    tone: Tone.Danger;
}
```

Опечатка в имени параметра или варианта, несовместимый тип, нарушение `#[range]`/`#[pattern]`
и пропущенное обязательное свойство дают диагностику. События объявляются отдельно:

```text
event open(id: String);
event edited(id: String, value: String);
```

Существующие `events: ['saveRequested'];` и события без аргументов поддерживаются.
Поля Rust ViewModel по-прежнему проверяет Rust-компилятор; их не нужно повторно
объявлять в `.ui`.

## Локальное состояние (`state`), вычисляемые `let` и горячие клавиши (`shortcut`)

Помимо внешней Rust ViewModel, компонент может объявлять реактивное локальное
состояние `state`, вычисляемые привязки `let` и клавиатурные сокращения `shortcut`:

```text
component CounterCard {
    state count: Int = 0;
    state expanded: Bool = false;

    let doubled = count * 2;
    let label = 'Счёт: ${count} (x2 = ${doubled})';

    shortcut 'Mod+K' -> state.toggle('expanded');
    shortcut 'ArrowUp' -> state.increment('count', 1);

    Column {
        gap: 8;
        Text { text: label; }
        Button {
            text: '+1';
            clicked -> state.increment('count', 1);
        }
    }
}
```

В интерактивном предпросмотре Studio поддерживаются встроенные мутации локального
состояния (также доступные через `actions.*`):
- `state.toggle('field')` — инвертирует булево поле;
- `state.set('field', value)` — устанавливает новое значение (поддерживаются вложенные пути `user.name`);
- `state.increment('field', delta)` / `state.decrement('field', delta)` — изменяет числовое поле;
- `state.push('list', item)` / `state.remove('list', index)` — добавляет или удаляет элемент списка;
- `state.reset('field')` — сбрасывает поле (или всё состояние без аргументов) к начальному значению.

## Стили (`style` / `apply`), дизайн-токены (`tokens`) и локализация (`i18n`)

Повторяющиеся наборы свойств можно группировать в именованные блоки `style`
и подключать через `apply`. Дизайн-токены объявляются в блоке `tokens`,
а словари локализации — в блоках `i18n <locale>`:

```text
component DashboardCard {
    tokens {
        spacing.sm: 8;
        spacing.md: 16;
        colors.surface: #1e293b;
        colors.accent: #38bdf8;
    }

    i18n ru { title: 'Входящие'; }
    i18n en { title: 'Inbox'; }

    style cardSurface {
        padding: tokens.spacing.md;
        background: tokens.colors.surface;
        cornerRadius: 12;
    }

    prop unread: Int = 5;

    Frame {
        apply cardSurface;
        Text {
            text: '${i18n.title}: ${unread} ${plural(unread, "сообщение", "сообщения", "сообщений")}';
        }
    }
}
```

Явные свойства узла имеют приоритет над свойствами из `apply`.

## Именованные слоты (`slot` и `@slot`)

Помимо стандартного `ContentPresenter`, компоненты могут объявлять именованные
слоты `slot name { ... }` с содержимым по умолчанию, а экземпляры могут
передавать разметку в конкретный слот через `@name { ... }`:

```text
component DialogShell {
    Column {
        gap: 12;
        slot header {
            Text { text: 'Заголовок по умолчанию'; }
        }
        ContentPresenter {}
        slot footer {
            Row { Button { text: 'Закрыть'; } }
        }
    }
}

DialogShell {
    @header {
        Badge { text: 'Важно'; tone: 'accent'; }
    }
    Text { text: 'Основное содержимое диалога'; }
}
```

## Группы свойств, `match` в разметке и передача параметров

```text
match props.tone {
    Tone.Danger => {
        background: state.theme.dangerWash;
        borderColor: state.theme.dangerInk;
        icon: 'assets/error.svg';
    }
    _ => {
        background: state.theme.accentWash;
        borderColor: state.theme.accentInk;
        icon: 'assets/info.svg';
    }
}
```

Групповой `match` выбирает первую подходящую ветку и применяет её свойства
вместе. Кроме того, `match` можно использовать прямо на уровне разметки для
выбора дочерних узлов, а в образцах поддерживаются ИЛИ-шаблоны (`'a' | 'b'`)
и числовые диапазоны (`0..10`, `1..=5`):

```text
match state.status {
    'loading' | 'pending' => Spinner {}
    'error' => {
        Badge { text: 'Ошибка'; tone: 'danger'; }
        Button { text: 'Повторить'; }
    }
    _ => Text { text: 'Готово'; }
}
```

Приоритет в компоненте: значения по умолчанию → `apply` → выбранная группа →
явные свойства экземпляра. У дочернего элемента явно записанные свойства
имеют приоритет над группой и `forward`. Несколько выбранных групп не должны
задавать одно и то же свойство.

```text
FieldContent {
    forward props { value, placeholder, fontSize, icon };
    color: props.textColor;
}
```

`forward` передаёт только перечисленные одноимённые параметры из контекста
родительского компонента. Неизвестное имя — ошибка. Переименование записывается
обычным свойством. Автоматической передачи всех свойств нет.

## Изменение узла по ключу

Свойства унаследованного узла можно изменить по `key`, сохранив его тип и детей:

```text
component LargeButton : Button {
    override caption {
        fontSize: 18;
    }
}
```

Для ключа `ContentPresenter` изменяется корень содержимого. Полная замена
содержимого использует двоеточие: `override content: Text { ... };`.
Правила и примеры — в [описании наследования](vector-ui/COMPONENTS.md).

## Выражения, конвейеры (`|>`) и стандартная библиотека (`stdlib`)

```text
disabled: state.busy || !state.valid;
width: clamp(state.width - 24, 120, 480);
text: 'Найдено: ${state.count}';
text: "Найдено: ${state.count}";
placeholder: state.customer?.name ?? 'Имя';
let tagsText = state.tags |> filter('active') |> map('title') |> join(', ');
```

Обе разновидности кавычек имеют одинаковую семантику. `\${state.count}`
выводит текст `${state.count}` без подстановки. Поддерживаются `\n`, `\r`, `\t`,
экранирование кавычек и обратной косой черты.

Доступны:
- арифметика `+ - * / %`, сравнения, `!`, `&&`, `||`, `??`, тернарный оператор `condition ? value : alternative`;
- конвейерный оператор `value |> fn(arg2, ...)` (передаёт левый операнд первым аргументом функции) и вызов методов через точку (`text.trim().upper()`);
- диапазоны `start..end` и `start..=end` (в `for i in 0..5` и в ветках `match`);
- литералы списков `[1, 2, 3]` и объектов/словарей `{ a: 1, b: 2 }`;
- единицы измерения длины (`px`, `%`, `rem`, `em`, `vw`, `vh`), времени (`ms`, `s`) и углов (`deg`, `rad`, `turn`).

Встроенные чистые функции стандартной библиотеки:
- **Математика**: `min`, `max`, `clamp`, `abs`, `round`, `floor`, `ceil`, `pow`, `sqrt`, `sign`, `lerp`, `remap`, `sum`, `avg`;
- **Строки и локализация**: `len`, `trim`, `upper` (`toUpperCase`), `lower` (`toLowerCase`), `capitalize`, `contains`, `startsWith`, `endsWith`, `replace`, `slice`, `split`, `padStart`, `padEnd`, `repeat`, `truncate`, `format`, `plural`;
- **Списки и словари**: `count`, `first`, `last`, `join`, `reverse`, `take`, `skip`, `map`, `filter`, `sort`, `unique`, `flatten`, `chunk`, `indexOf`, `range`, `keys`, `values`, `entries`, `merge`, `pick`, `omit`, `get`;
- **Предикаты и приведение типов**: `String`, `Number`, `Bool`, `isEmpty`, `isNull`, `coalesce`;
- **Цвет и доступность (WCAG)**: `rgb`, `rgba`, `hsl`, `hsla`, `alpha`, `lighten`, `darken`, `mix`, `luminance`, `contrast`, `invert`, `complement`.

Строковая интерполяция принимает скалярные значения. Неизвестная функция или
отсутствующее обязательное поле диагностируются. Для вычислений с побочными
эффектами используются обработчики в Rust или действия `state.*` в предпросмотре.

## Условия и списки

```text
Column {
    gap: 8;

    if state.loading {
        Skeleton {}
    } else {
        Text { text: 'Готово'; }
    }

    for item in state.items key item.id {
        Button {
            text: item.title;
            clicked -> events.open(item.id);
        }
    } empty {
        Text { text: 'Список пуст'; }
    }
}
```

`if` создаёт выбранную ветку. `for` создаёт элементы по коллекции, `empty`
задаёт содержимое для пустой коллекции. Ключ — строка или конечное число,
уникальное в пределах этого повторителя. Ключи разных типов различаются.
Идентичность включает место повторителя и ключ, поэтому перестановка
коллекции сохраняет фокус и состояние оставшихся контролов. Удаление
сфокусированного элемента снимает фокус. Повторные ключи — ошибка.

Вложенные повторители используют разные имена переменных. В Rust коллекция
может быть `Property<Vec<Row>>`, где `Row: Clone + PartialEq`. Генератор
извлекает используемые поля без ручного преобразования всей модели в словарь.

```text
for item in state.items key item.id {
    TextField {
        value <-> item.title;
        changed -> events.edited(item.id, item.title);
    }
}
```

Запись в модель происходит перед событием `changed`; аргументы события
соответствуют актуальной строке и новому значению. Подробности подписок,
перепривязки контекста и Rust API — в [BINDINGS.md](vector-ui/BINDINGS.md).

## Раскладка

- `Row`: последовательность по горизонтали.
- `Column`: последовательность по вертикали.
- `Grid`: сетка с автоматическим размещением свободных элементов.
- `Stack`: наложение элементов.

```text
Row {
    gap: 8;
    if props.showIcon {
        Icon { width: 16; height: 16; source: props.icon; }
    }
    FieldValue { width: *; minWidth: 80; }
}

Grid {
    columns: [160, *];
    gap: 8;
    Button { text: 'Первый'; }
    Button { text: 'Второй'; }
    Button { text: 'Следующая строка'; }
}
```

В `Row`/`Column` основной размер по умолчанию определяется содержимым;
`*` и `2*` делят оставшееся место. Поперечный размер без указания растягивается,
явно заданный размер центрируется. Учитываются `minWidth`, `maxWidth`,
`minHeight`, `maxHeight`; минимум не может превышать максимум.
`auto`, `content` и `-` используют размер по содержимому, включая метрики текста.

В `Grid` явные `cell`/`row`/`column` занимают свои ячейки; остальные элементы
заполняют свободные места по строкам. При необходимости добавляются строки
по содержимому. Поддерживаются `row.span` и `column.span`.

Прежний `Frame` сохраняет совместимость: корневой Frame без сетки размещает
контролы вертикально, Frame внутри визуального компонента накладывает детей.
Явные `Row`/`Column`/`Grid`/`Stack` позволяют выразить нужный режим без этого
контекстного различия. Векторная сцена по-прежнему ограничена 256 контролами.
Обрезка корня и внутренних визуальных частей поддерживается; вложенные
контейнеры сцены с отдельным `clip` требуют дальнейшей поддержки renderer.

`Modal { key; x; y; width; height; backdrop; dismiss; open; dismissed -> actions.name(); … }`
объявляет модальный слой после контролов сцены. Без `x`/`y` слой центрируется
в корне; дети размещаются внутри его поверхности как в `Column`, а в свободной
раскладке сохраняют свои `x`/`y` относительно слоя. Открытый слой получает весь
ввод: контролы под backdrop недоступны, Tab ходит по кругу, Escape и нажатие вне
поверхности вызывают `dismissed`. Подробности и API runtime описаны в
[README векторного среза](vector-ui/README.md).

## Примеры

Запускаемый [Rust-пример со списком задач](vector-ui/examples/language-app/README.md)
показывает типизированные свойства и события, группы, `forward`, выражения,
редактирование и перестановку строк, `Row` и `Grid`:

```sh
cargo run --locked --manifest-path vector-ui/examples/language-app/Cargo.toml
```

Обычные `Text` и `TextInput` работают в сцене без файлов компонентов.
При наличии `components/Text.ui` или `components/TextInput.ui` используется
определение проекта. Внутри визуального дерева они остаются примитивами.
Кроме того, в Studio и библиотеке контролов доступны готовые визуальные
компоненты `Badge`, `Tag`, `Chip`, `Avatar`, `ProgressBar`, `Divider` и `Separator`.

В библиотеке [Alert](vector-ui/controls/components/Alert.ui) использует
групповой `match`, [TextField](vector-ui/controls/components/TextField.ui) —
`forward`, [FieldContent](vector-ui/controls/components/FieldContent.ui) —
`Row` с условной иконкой без дублирования поля ввода.

