# Forma — библиотека интерфейса

Rust crate называется `forma` и подключается в приложение как библиотека:
`use forma::Runtime;`. Каталог исходников остаётся `vector-ui`.
`Runtime::from_sources(scene, template)` принимает разметку и шаблон компонента.
Пример подключения и схема версий — [README проекта](../README.md#библиотека-forma)
и [релизы](../RELEASING.md). Нативный demo-бинарник называется `forma`.

Генерируемые Rust-формы, наследуемый контекст, наблюдаемые поля и типизированные
события — [BINDINGS.md](BINDINGS.md). `npm run example:bindings:native` запускает
отдельный пример с редактированием и сменой ViewModel.

Контракты `prop`/`event`, enum, групповой `match`, `forward`, выражения,
условия, списки с ключами и раскладка `Row`/`Column`/`Grid`/`Stack` описаны
в [спецификации языка](../LANGUAGE.md). Запускаемый пример всех этих возможностей —
[language-app](examples/language-app/README.md).

## Runtime с несколькими контролами

Studio/WASM и native используют общий `Runtime`: один Frame с несколькими
кнопками, независимые hover/pressed/focus/click и переход фокуса Tab/Shift+Tab.
Низкоуровневая сцена использует Frame; компилятор размещает вложенные
`Row`/`Column`/`Grid`/`Stack` и передаёт runtime координаты контролов.
Один Scroll может охватывать всю группу. В хосте снимков события выводятся в журнал. Формы с `contextType` запускаются
из Studio через генерацию с живой Rust ViewModel; см. [привязки](BINDINGS.md).
История этапов — [этап 2б](ARCHITECTURE_STEPS.md).

Пример: `examples/TwoButtons.ui`. С запущенным `npm run dev` откройте
[стенд двух кнопок](http://127.0.0.1:5173/vector-ui/examples/runtime.html).
Native: `cargo run --offline --release --manifest-path vector-ui/Cargo.toml --features native -- vector-ui/examples/TwoButtons.ui vector-ui/examples/Button.component.ui`
(из корня репозитория).

В заголовке native-окна показаны backend, FPS и средний интервал между
успешными `present` за последнюю секунду. Это частота передачи кадров оконной
системе, а не время вычисления GPU или подтверждённые обновления монитора.
Через секунду без кадров отображается `idle`; скрытая поверхность — `occluded`.
В обычном режиме окно перерисовывается при изменениях и во время анимаций.
**F8** включает/выключает непрерывную перерисовку для замера FPS.
Для диагностики при запуске: `FORMA_FPS_TEST=1` включает этот режим сразу,
`FORMA_FPS_LOG=1` дублирует изменения показаний в stdout.

Ниже сохранено описание исходного одноконтрольного среза; сведения о его
ограничении одной кнопкой и только CPU устарели. Текущий backend — WebGPU/native
wgpu с CPU fallback.

# Векторный вертикальный срез

## Работа через Forma Studio

Переполнение — допустимая раскладка: Frame по умолчанию использует `overflow: visible;`, `overflow: hidden;` обрезает изображение и hit-testing по границам Frame. Padding не обязан вместе с дочерним размером помещаться в Frame.

Прокрутка задаётся отдельным элементом, не значением overflow:

```css
Frame {
    width: 300;
    height: 100;
    padding: 10;
    Scroll {
        Button { width: 400; height: 160; text: 'Большая кнопка'; }
    }
}
```

В текущем векторном срезе Scroll занимает внутреннюю область Frame за вычетом padding и содержит одну Button. Колесо/тачпад прокручивает по двум осям; индикаторы рисует Rust. Перетаскивание индикатора и вложенные Scroll ещё не реализованы. `visible` в дизайнере расширяет область рисования, не меняя заданные размеры Frame; за пределами окна ОС рисовать невозможно, поэтому native-срез открывает окно по области рисования.

Brush — значение свойства: `Rectangle { background: Brush { color: props.background; hover: #a8baff; transition: 140ms; }; }`. У Border используется `background: Brush { color: #bed0ff; };`. Одноцветные сокращения: `background: #8ca5ff;`, `background: #bed0ff;`. Самостоятельные дочерние узлы Brush больше не принимаются.

В дереве виртуального проекта:

- `ui/VectorButton.ui` — экземпляр Button внутри Frame: размеры, padding, text и clicked.
- `components/Button.ui` — шаблон компонента: Rectangle, Brush, Border, Text, PointerArea.

Откройте экземпляр → «Показать UI» → «Вектор · Rust/WASM». Затем откройте компонент: его изменения сразу перерисовывают экземпляр. «▶ Взаимодействие» включает hover/pressed и события. «▶ Приложение» запускает те же два .ui-снимка через native Rust, без WebView. Код примера в репозитории: `examples/Button.ui` и `examples/Button.component.ui`.

`props.*` берёт значения экземпляра. Brush задаёт color и необязательные hover/pressed/disabled/focus, `transition: 140ms;` — плавную интерполяцию цвета при смене состояния. Border рисует внутреннюю границу собственной Brush. PointerArea `clicked -> events.clicked();` передаёт событие экземпляру; без этой строки событие не возникает. Нет выполнения произвольного кода из шаблона.

Текущий строгий срез: один Frame и один Button, один Rectangle с необязательными Brush/Border/Text/PointerArea. Это ещё не произвольный реестр компонентов или полная система layout. Неподдерживаемые узлы и свойства дают ошибки, а не скрытый HTML fallback. ViewModel, design-файлы и прикладной Rust-обработчик не подключены к этому срезу; clicked записывается в журнал. `preview_renderer` / `vector_status` и обычный `app_run` доступны через MCP.

Одна кнопка: геометрия, покрытие пикселей (4 samples), состояния и hit-test реализованы в Rust без Vello, SVG и HTML-контролов. Native использует winit + softbuffer только для окна и показа буфера. Web использует WASM + canvas putImageData для показа того же буфера. Это CPU-растеризация, пока не GPU renderer.

Native: `cargo run --manifest-path vector-ui/Cargo.toml --features native`

Web (из корня репозитория):

```sh
cargo build --manifest-path vector-ui/Cargo.toml --target wasm32-unknown-unknown --lib --release
wasm-bindgen vector-ui/target/wasm32-unknown-unknown/release/forma.wasm --target web --out-dir public/vector-pkg
npm run dev
```

Результат — `public/vector-pkg/forma.js` и `public/vector-pkg/forma_bg.wasm`.
Открыть `/vector.html`. wasm-bindgen CLI должен быть версии 0.2.125, как зависимость ядра.

Текст Button рисуется из контуров встроенного Ubuntu-Light.ttf собственным scanline-растеризатором; ttf-parser только читает шрифт. Лицензия шрифта — assets/UFL.txt. Поддержаны Latin/Cyrillic без сложного shaping/kerning. Нет TextInput/IME, полноценного accessibility и дерева фокуса. Native проверен сборкой на macOS; другие ОС не проверены. Снимки запуска в .forma/vector-* сохраняются для диагностики.
# Свойства компонента

В `examples/Button.component.ui` значения по умолчанию объявлены в начале
`component Button`, перед `Rectangle`. Размеры, текст, цвета состояний, рамка
и длительность перехода доступны примитивам через `props.*`.
Например: `fontSize: 16;` → `Text { fontSize: props.fontSize; }`.

Приоритет: явно заданное свойство экземпляра → значение компонента → резервное
значение движка. `Button { width: 60; }` переопределяет `width: 100;` компонента.
Ноль и `false` также считаются явными переопределениями. Значения компонента
пока только литеральные; произвольные пользовательские свойства не реализованы.
`fontSize` — основное имя, `font.size` оставлено совместимым псевдонимом.
