# Контролы Forma

Компоненты для общей Rust/WASM-библиотеки `forma`: кнопки, выбор, поля,
диапазоны, навигация, дерево, таблица, сообщения и составные оболочки.
Образцы рисует Forma; HTML используется для оболочки каталога.
**Полный перенос референса ещё не завершён.** Визуальные части, модели состояния
и поведение хостов имеют разный статус — подробная карта находится в
[REFERENCE_COVERAGE.md](REFERENCE_COVERAGE.md).

## Каталог и запуск

Из корня репозитория:

```sh
npm ci
npm run build:wasm
npm run dev
```

Откройте [каталог контролов](http://127.0.0.1:5173/vector-ui/examples/controls.html).
Для `build:wasm` нужен CLI wasm-bindgen 0.2.125, соответствующий зависимости
Rust: `cargo install wasm-bindgen-cli --version 0.2.125 --locked`.

Каталог переключает светлую/тёмную тему, четыре размера кнопок и reduced motion.
«Без анимаций» учитывает `prefers-reduced-motion` и допускает ручное изменение.
Tab/Shift+Tab перемещают фокус, Space/Enter активируют доступные контролы.
Для групп и дерева работают стрелки; Slider поддерживает drag, стрелки,
Home/End и PageUp/PageDown. События также показываются в журнале.

| Слой | Что работает |
|---|---|
| Визуальные части | Независимые `.ui`, наследование, слоты, SVG, темы, состояния, явные переносы строк |
| Общий Rust Runtime | Hover/pressed/focus/disabled/click, цветовые переходы и Reveal; CPU/GPU и WASM/native используют общую реализацию |
| Rust-модели | Check, Toggle, Range, SingleSelection, Tree и TextEdit (одна или несколько строк); доступны в `forma::control_state` и через WASM-обёртки |
| Галерея | Подключённый выбор, группы, диапазон/stepper, дерево, закрытие вкладок, select/disclosure, локальный dialog, toast timer и изменение ширины sidebar через Splitter |
| Экспорт Studio/native | Самостоятельный визуальный снимок и базовый Runtime; обработчики `catalog-session.js` в экспорт не входят |

TextField/SearchField/TextArea содержат TextInput. Общий Rust Runtime хранит
текст, рисует каретку и выделение, обрезает и прокручивает содержимое.
Работают набор, drag/Shift+стрелки, Home/End, Backspace/Delete, Select All,
Undo/Redo и Enter в Textarea. Web-хост передаёт beforeinput, clipboard и
composition; native winit — клавиатуру и IME, clipboard подключён на macOS.
Значение доступно через Runtime.text_value(index); прикладные bindings не нужны
для самого редактирования. Перезапуск окна начинает с value из разметки.
Dialog работает внутри своей секции: это локальный пример с Tab-cycle,
Escape и возвратом focus, а не модальный менеджер всех окон и секций.

## Studio и native

«Скачать пример для Studio» сохраняет JSON-проект с текущей темой, размером,
настройкой анимаций и текущим визуальным состоянием контролов галереи.
Обработчики и работающие модели сессии в экспорт не входят. Импортируйте проект в Studio,
выберите `ui/FormaControls.ui`, «Показать UI» и «Вектор · Rust/WASM».
«▶ Приложение» запускает снимок в нативном окне.

Пример можно подготовить через CLI:

```sh
npm run example:controls -- dark touch
cargo run --offline --release --manifest-path vector-ui/Cargo.toml --features native -- .forma/controls/scene.ui .forma/controls/template.ui
```

`example:controls` принимает тему, размер и необязательный выходной каталог;
по умолчанию `light regular .forma/controls`. Команда создаёт `scene.ui`,
`template.ui` и `project.json` с исходными демонстрационными значениями.
Предварительная WASM-сборка нужна и здесь:
ширину текста измеряет тот же Rust-код, который его рисует.

## Состав и композиция

| Группа | Контролы и части |
|---|---|
| Основа | `Surface`, `Button`, `Card`, `Separator`, `ColorSwatch` |
| Кнопки | `PrimaryButton`, `SecondaryButton`, `GhostButton`, `DangerButton`, `IconButton`, `LeadingIconButton`, `PrimaryIconButton`, `LoadingButton` |
| Выбор | `Checkbox`, `RadioButton`, `Switch`, `Chip`, `SegmentButton`, `TabButton`, `FormattingButton` |
| Поля и диапазон | `TextField`, `SearchField`, `TextArea`, `SelectTrigger`, `Slider`, `NumberValue` |
| Навигация | `NavigationItem`, `PageButton`, `TreeItem`, `MenuItem`, `DisclosureHeader` |
| Данные и сообщения | `TableHeader`, `TableRow`, `CodeLine`, `Badge`, `Avatar`, `ProgressBar`, `Alert`, `Toast`, `Tooltip`, `EmptyState`, `Skeleton`, `Spinner` |
| Оболочки | `WindowHeader`, `ToolbarSurface`, `StatusBar`, `DialogSurface`, `Splitter`, `StatusDot`, `Keycap` |
| Визуальные части | `Label`, `StartLabel`, `BodyText`, `Paragraph`, `Icon`, `IconLabel`, `MessageContent`, `CheckMark`, `ChoiceIndicator`, `ChoiceContent`, `SwitchThumb`, `TreeContent`, `FieldValue`, `FieldContent`, `MultilineValue`, `SliderTrack`, `TableCells` |

Каждый исходник находится в [components](components). `Surface` содержит один
корневой Rectangle с Brush/Border и слоты `reveal`, `content`, `interaction`.
`Button` заменяет interaction на PointerArea. Варианты наследуют эту структуру;
составные визуальные части размещают Icon/Label/Rectangle в Frame.
Пассивные Surface-наследники не участвуют в Tab и не генерируют click.

[patterns.js](patterns.js) собирает соседние контролы: `choiceGroup`,
`buttonGroup`, `numberStepper`, `dataTable`, `avatarGroup`, `breadcrumb`,
`documentTabs`, `dialog`, `codeBlock`. Это функции построения списка экземпляров,
а не скрытые обработчики или полноценные Table/Window/Editor-модели.

В AppShell галереи Splitter меняет ширину sidebar перетаскиванием,
ArrowLeft/ArrowRight и Home/End. `RangeValue` ограничивает её диапазоном
70–136 px, исходная ширина — 90 px. Native-хост связывает ту же модель
диапазона со своим layout и событиями окна.

После подключения файлов библиотеки компонент приложения выглядит так:

```css
component Demo {
    Frame {
        width: 380; height: 220; padding: 24; gap: 16;
        PrimaryIconButton {
            key: 'create'; width: 300;
            text: 'Новый проект'; icon: 'assets/plus.svg';
            clicked -> actions.create();
        }
        Checkbox { key: 'autosave'; text: 'Автосохранение'; checked: true; }
        ProgressBar { width: 300; value: 60%; }
    }
}
```

`actions.create` обозначает действие, которое получает хост; произвольный код
приложения из `.ui` пока не вызывается. Checked/selected/expanded/value задаются
снимком. Галерея меняет их через модели и повторную компиляцию нужной секции.
`Slider.value` и `ProgressBar.value` — проценты (`60%`, не число `60`);
у Slider один value задаёт и заливку, и позицию ручки. Rust Runtime обрабатывает перетаскивание, стрелки, PageUp/PageDown и Home/End в WASM и нативном окне. Числовые подписи и отдельные кнопки шага требуют подключения к общей модели приложения.

| API состояния | Применение |
|---|---|
| `checked`, `indeterminate` | Checkbox; RadioButton использует checked |
| `checked` | Switch |
| `selected` | Chip, SegmentButton, TabButton, NavigationItem, PageButton, MenuItem |
| `branch`, `expanded`, `selected`, `indent` | TreeItem; отступ задаётся в пикселях, иконка — через `icon` |
| `value`, `placeholder`, `error`, `suffix`, `showIcon`, `icon` | TextField/SearchField; value — строка |
| `value`, `placeholder` | TextArea; явные переносы строк сохраняются |
| `value`, `expanded` | SelectTrigger/DisclosureHeader; подпись берётся из value |
| `tone` | Badge: neutral/accent/success/warning/danger; Alert: accent/success/warning/danger |

У Alert `text` — заголовок, `description` — описание, `icon` заменяет SVG.
У IconButton задавайте осмысленный `text`, даже когда подпись не нарисована.
Текстовое имя — часть будущего accessibility-контракта, но само по себе не
создаёт OS/browser accessibility-узел.

Собственный вариант создаётся наследованием и заменой слота:

```css
component NextButton : SecondaryButton {
    text: 'Продолжить'; icon: 'assets/arrow-right.svg';
    override content: IconLabel {
        icon: props.icon; iconSize: props.iconSize;
        horizontalPadding: props.horizontalPadding;
        text: props.text; color: props.textColor; fontSize: props.fontSize;
    };
}
```

Вложенные компоненты имеют собственные props; значения передаются явно.
Их корень может быть Frame, Text, Image или Rectangle. Внутрь помещаются
визуальные части; вложенные PointerArea, события и bindings не поддерживаются. TextInput —
специализированная часть редактора (не более одной на контроль).
Интерактивные контролы остаются соседями верхнего Frame/Scroll.

В [assets](assets) лежат геометрические SVG для действий, сообщений, выбора,
навигации, файлов и загрузки. Они используют currentColor. Произвольные path,
transforms, фильтры, SVG-текст и неподдержанные stroke-атрибуты загрузчик
не принимает; новые иконки следует проверять через `src/svg-shapes.js`.

## Темы, размеры и компиляция

[tokens.js](tokens.js) экспортирует `themes.light`/`themes.dark`, `sizes`,
`themeState()` и `materializeTheme()`. Компоненты используют `state.theme.*`,
`state.motion.control` и `state.motion.reveal` как именованные константы.
`materializeTheme(libraryFiles, 'dark', { reducedMotion: true })` подставляет
конкретные цвета и длительности в `.ui`, сохраняя SVG.

`libraryFiles` — объект текстов по путям `components/Surface.ui`,
`components/Button.ui`, `assets/search.svg` и остальным относительным путям.
Общий linker применяется ко всем исходникам и точке входа:

```js
import { compileComponents } from './src/components.js';
import { materializeTheme } from './vector-ui/controls/tokens.js';

const entry = 'ui/Demo.ui';
const files = materializeTheme({ ...libraryFiles, [entry]: source }, 'dark');
const compiled = compileComponents(files, entry, {}, {
    measureText: wasm.text_metrics,
});
```

Здесь `wasm` — инициализированный модуль `forma.js`. `compiled.source` и
`compiled.template` передаются в WASM Runtime через `load_component` или в Rust
через `forma::Runtime::from_sources`. Linker заранее разрешает наследование,
слоты и вложенные визуальные части; Rust получает готовые примитивы.

[catalog.js](catalog.js) экспортирует `sectionSource(id, theme, size, state)`,
`catalogSource(theme, size, state)` и `catalogProject(libraryFiles, theme, size, options)`.
Последняя функция возвращает карту файлов для Studio с `ui/FormaControls.ui`.
`options.state` задаёт экспортируемый снимок; без него используются исходные
демонстрационные значения. Галерея передаёт текущий `session.state()`.
[gallery.js](gallery.js) связывает эти функции с
[catalog-session.js](catalog-session.js), где расположено демонстрационное
поведение Rust-моделей. Это пример интеграции хоста, не общий движок bindings.

| Размер | Высота | Шрифт | Padding по горизонтали | Радиус | Иконка |
|---|---:|---:|---:|---:|---:|
| `compact` | 28 | 13 | 10 | 6 | 14 |
| `regular` | 32 | 14 | 12 | 8 | 16 |
| `comfortable` | 40 | 15 | 16 | 9 | 18 |
| `touch` | 44 | 16 | 16 | 10 | 20 |

Это наборы явных свойств экземпляра, не глобальный size-binding.
Переключатель каталога применяется к базовым примерам кнопок; составные
секции имеют собственные размеры. Ширину выбирает потребитель.
Цветовой переход — 180 ms, Reveal — 280 ms; reducedMotion задаёт им 0 ms.

## Reveal и адаптация референса

Источник — `Unified Style Guide.dc.html` и screenshots из
`.forma/references/unified-style-guide`. Тёплые нейтрали, акцент и основные
роли сохранены; текущий HTML принят за основу, поскольку изображения отражают
разные редакции.

Reveal реализован и проверен в общем Rust renderer: пятно на рамке появляется
рядом с указателем, а при входе расширяется с radius 120/stop 60% до
radius 1200/stop 99%. Это rv2 из референса. CPU/GPU используют один paint;
проверены состояния и DPI. Focus даёт сплошную рамку, disabled отключает эффект.
`revealWidth`/`revealColor` на Surface позволяют настроить его; нулевая ширина
отключает. Slider задаёт `revealWidth: 1`, сохраняя видимую рамку фокуса при
нулевой обычной границе. Это отдельный эффект от блика внутри заливки и entrance-анимаций.

Для текста на кнопках выделены роли `primary*` и `danger*`. В светлой теме
белый на исходном `#DE6A19` давал 3,40:1; основная кнопка использует `#B9510F`
и даёт 4,94:1, Danger — `#C23625` и 5,46:1. Декоративный accent сохраняет
исходный оттенок. Для меток и сообщений разделены wash и читаемый foreground,
включая dangerInk. Это проверка конкретных пар, не всей доступности приложения.

Цели 18/22 px из референса не стали базовыми: compact начинается с 28,
touch — с 44. Автоматического выбора touch-метрик нет. Новые символы плавно
проявляются за 220 ms; значение и положение каретки обновляются сразу.
Остальные дополнительные эффекты — shine,
ripple, fill glow, shadows, spinner rotation, shimmer и transitions overlays —
перечислены отдельно в карте покрытия; наличие Reveal не означает их готовность.

## Оставшаяся работа

- **Текст:** автоматический wrap, grapheme/word navigation, blink каретки,
  IME surrounding-text/reconversion и clipboard на Windows/Linux ещё нужны.
  Каретка плавно мигает (цикл 1100 ms), новые символы проявляются за 220 ms без задержки ввода. При reduced motion каретка постоянная, символы появляются сразу. Составные Unicode-графемы не объединяются:
  перемещение идёт по Unicode scalar, позиции — UTF-8 bytes. История — 100 правок.
- **RTF:** документ с диапазонами стилей, bold/italic/underline/strike,
  headings/lists, clear formatting, clipboard formats и история. Переключение
  FormattingButton в галерее ещё не форматирует текст.
- **Native-хост:** связать модели с событиями окна, обновлением снимка,
  capture/focus и жизненным циклом окон. Экспорт `.ui` не переносит JS-сессию.
- **Overlays:** общий слой с anchor/viewport positioning, z-order/clipping,
  outside-click, блокировкой фонового ввода и modal semantics; tooltip delay,
  toast queues и native focus/window routing. Локальные примеры уже работают.
- **Текст и данные:** renderer использует встроенный Ubuntu Light; выбор
  шрифтов/весов, автоматический wrap, ellipsis и сложный shaping ещё нужны.
  Явные переводы строк поддержаны. Table/CodeLine/Shell остаются композициями;
  sorting, virtualization, syntax editor и terminal ещё не готовы. Splitter
  работает в галерее; его связь с native-layout и docking остаётся у хоста.
- **Accessibility:** отдельное дерево ролей/имён/значений/состояний в web и
  native, label/helper/error relations, group semantics, live announcements
  и focus notifications. Общая подпись canvas и Tab не заменяют эти адаптеры.
- **Прикладной контракт:** типы, стили, зависимости, bindings и подключение
  пользовательского кода остаются для отдельного обсуждения.

Полный inventory референса с отдельными статусами галереи и native-хоста,
точной механикой Reveal и списком motion — в
[REFERENCE_COVERAGE.md](REFERENCE_COVERAGE.md).
