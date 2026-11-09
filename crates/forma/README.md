# Forma — библиотека интерфейса

Rust crate называется `forma` и подключается в приложение как библиотека:
`use forma::Runtime;`. Каталог исходников остаётся `crates/forma`.
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
[стенд двух кнопок](http://127.0.0.1:5173/crates/forma/examples/runtime.html).
Native: `cargo run --offline --release --manifest-path crates/forma/Cargo.toml --features native -- crates/forma/examples/TwoButtons.ui crates/forma/examples/Button.component.ui`
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

### Модальные слои

```css
component Confirm {
    Frame {
        width: 360; height: 220;
        Button { key: 'open'; text: 'Открыть'; clicked -> actions.open(); }
        Modal {
            key: 'ask'; x: 40; y: 30; width: 280; height: 160;
            backdrop: #00000080; dismiss: true; open: true;
            dismissed -> actions.cancel();
            Button { key: 'yes'; x: 16; y: 100; width: 120; height: 40; text: 'Да'; clicked -> actions.yes(); }
        }
    }
}
```

`Modal` объявляется после контролов Frame или Scroll; слоёв может быть несколько,
ввод получает верхний открытый. Координаты детей относительны поверхности слоя,
слои не прокручиваются и не расширяют содержимое сцены. Общий Rust Runtime рисует
backdrop над сценой (CPU и GPU), не даёт контролам под ним hover, hit и фокус,
зацикливает Tab внутри слоя, закрывает верхний слой по Escape и по нажатию вне
поверхности (`dismiss: false` отключает оба пути), возвращает фокус контролу,
который был активен до открытия, и запоминает состояние слоя при перезагрузке
сцены с тем же `key` и тем же объявленным `open`. `open_modal`, `close_modal` и
`dismiss_modal` меняют состояние во время работы; `dismiss_count`,
`dismissed_index` и `modal_action` позволяют хосту выполнить обработчик
`dismissed`. Web-хост передаёт Escape (код клавиши 3) и нажатия по backdrop,
native-окно — Escape и щелчки мыши; события выводятся в журнал так же, как `clicked`.
Anchor-позиционирование немодальных popup, задержки tooltip и очереди toast в
слой не входят.

Brush — значение свойства: `Rectangle { background: Brush { color: props.background; hover: #a8baff; transition: 140ms; }; }`. У Border используется `background: Brush { color: #bed0ff; };`. Одноцветные сокращения: `background: #8ca5ff;`, `background: #bed0ff;`. Самостоятельные дочерние узлы Brush больше не принимаются.

В дереве виртуального проекта:

- `ui/VectorButton.ui` — экземпляр Button внутри Frame: размеры, padding, text и clicked.
- `components/Button.ui` — шаблон компонента: Rectangle, Brush, Border, Text, PointerArea.

Откройте экземпляр → «Показать UI» → «Вектор · Rust/WASM». Затем откройте компонент: его изменения сразу перерисовывают экземпляр. «Взаимодействие» включает hover/pressed и события во встроенном preview. «▶ Приложение» запускает те же два .ui-снимка через native Rust, без WebView. Код примера в репозитории: `examples/Button.ui` и `examples/Button.component.ui`.

`props.*` берёт значения экземпляра. Brush задаёт color и необязательные hover/pressed/disabled/focus, `transition: 140ms;` — плавную интерполяцию цвета при смене состояния. Border рисует внутреннюю границу собственной Brush. PointerArea `clicked -> events.clicked();` передаёт событие экземпляру; без этой строки событие не возникает. Нет выполнения произвольного кода из шаблона.

Текущий строгий срез: один Frame и один Button, один Rectangle с необязательными Brush/Border/Text/PointerArea. Это ещё не произвольный реестр компонентов или полная система layout. Неподдерживаемые узлы и свойства дают ошибки, а не скрытый HTML fallback. ViewModel, design-файлы и прикладной Rust-обработчик не подключены к этому срезу; clicked записывается в журнал. `preview_renderer` / `vector_status` и обычный `app_run` доступны через MCP.

Одна кнопка: геометрия, покрытие пикселей (4 samples), состояния и hit-test реализованы в Rust без Vello, SVG и HTML-контролов. NativeHost desktop/Svet использует winit + GPU surface/vector Renderer; CPU softbuffer остаётся fallback и compatibility presentation. Web CPU API использует WASM + canvas putImageData. Реальный macOS GPU→CPU→GPU переход проверен отдельно от offscreen GPU tests.

Native: `cargo run --manifest-path crates/forma/Cargo.toml --features native`

Web (из корня репозитория):

```sh
cargo build --manifest-path crates/forma/Cargo.toml --target wasm32-unknown-unknown --lib --release
wasm-bindgen target/wasm32-unknown-unknown/release/forma.wasm --target web --out-dir public/vector-pkg
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

### Native application observation

With the `native` feature, `native_window::NativeApp` separates `update` (input
and application synchronization) from `inspect` (read the retained Runtime).
Implement `inspect(&self, observer: &mut dyn FnMut(&Runtime)) -> Result<(), String>`
by calling the observer exactly once on the current Runtime, without dispatching
input or synchronizing/replacing the application. IME presentation uses this
read path so caret and editor ownership refer to the runtime retained after
update has finished. `Runtime` implements both methods directly; wrapper apps
can follow `examples/native_input_probe.rs`. This required method must be added
when migrating existing custom NativeApp implementations.

`NativeApp::on_ime_event` is an optional diagnostic observer called before host
IME dispatch. The manual native_input_probe writes a separate `*.ime.jsonl`
stream beside its fixture state log when `FORMA_NATIVE_INPUT_PROBE_LOG` is set.
This includes preedit/commit text and is intended for the controlled fixture;
normal applications need not implement the hook or collect text.

### Хост для приложения

`NativeWindowConfig::default()` рассчитан на демо и пробы: читает `std::env::args`
и `FORMA_BACKEND` / `FORMA_PLATFORM`, без выбранного бэкенда показывает экран выбора,
перехватывает F6/F7 (а с `native-test-hooks` ещё F8/F9), дописывает к заголовку
бэкенд и подсказки, рисует акцентную полосу профиля egui и печатает строки запуска.
Это режим `HostMode::Demo`, он остаётся значением по умолчанию.

Приложение, встраивающее Forma, берёт `NativeWindowConfig::application("Заголовок")`
(режим `HostMode::Application`): аргументы и переменные окружения не читаются,
экрана выбора нет (`backend: None` означает Svet), функциональные клавиши доходят
до приложения и Runtime, заголовок окна совпадает с `title` дословно, полоса не
рисуется, в stdout хост ничего не пишет. Остальные поля меняются как обычно:

```rust
use forma::native_window::{run, NativeWindowConfig};

run(app, NativeWindowConfig { width: 1280., height: 800., ..NativeWindowConfig::application("Shelf") })
```

Эмуляция мобильной рамки включается только явным `platform: TargetPlatform::Ios`
или `Android`; `application` ставит платформу цели компиляции.

#### Пробуждение из рабочего потока

`on_dirty` выдаёт `Box<dyn Fn()>`, привязанный к потоку окна. Для фоновой работы
хост один раз, при создании окна, вызывает `NativeApp::on_waker(NativeWaker)`.
`NativeWaker` — `Send + Sync + Clone`; рабочий поток кладёт результат в канал или
под мьютекс приложения и вызывает `waker.wake()`. Цикл окна на своём потоке
вызывает `update` (и тогда, когда окно скрыто и кадр не рисуется), затем
перерисовывает. Вызовы, пришедшие до обслуживания предыдущего, сливаются в один,
поэтому `update` должен забирать все накопленные результаты. `wake()` возвращает
`false`, когда цикл окна завершён.

#### Обработчики приложения

У всех методов есть реализация по умолчанию, поэтому существующие `NativeApp`
компилируются и ведут себя как раньше.

| Метод | Когда вызывается | По умолчанию |
|---|---|---|
| `on_key_event(&mut self, &NativeKeyEvent) -> Result<bool, String>` | до передачи клавиши в Runtime; `true` — обработано, Runtime её не получит | `false` |
| `on_resized(&mut self, width, height, scale)` | при создании окна, изменении размера и масштаба; размеры логические | ничего |
| `on_close_requested(&mut self) -> Result<bool, String>` | запрос закрытия окна; `false` оставляет окно открытым | `true` |
| `on_pointer_event(&mut self, &NativePointerEvent)` | правая и средняя кнопки, двойной щелчок основной | ничего |

`NativeKeyEvent` несёт логическую и физическую клавиши, текст, `pressed`, `repeat`,
модификаторы (`command` — Command на macOS и Control на остальных системах) и
признак `composing`: во время набора через IME клавиша обычно принадлежит методу
ввода. В режиме `Demo` клавиши, перехваченные хостом, и ввод на экране выбора
бэкенда приложению не предлагаются.

Runtime по-прежнему получает только основную кнопку. Двойной щелчок сообщается
после того, как Runtime получил второе нажатие: интервал до 500 мс, смещение до
4 логических пикселей, третье нажатие начинает новую пару. Координаты событий
указателя — логические пиксели области содержимого окна. Хост не меняет размер
корня документа сам: это делает приложение в `on_resized`.

For native source/project operations, use the String-error APIs
`load_component_native`, `load_native`, `load_ui_files_json_native`,
`load_scene_artifact_native`, and `compile_ui_scene_artifact_native`.
The JS-facing exports remain adapters returning JsValue errors; invoking their
error paths on a non-WASM target can abort in wasm-bindgen. Native reload
refusals preserve the current scene; successful markup reload replaces the
retained document, while UI-project reload retains the scheduler selection.

The native touch adapter admits one primary `(device, touch id)` gesture at a
time. Other fingers cannot move or release that pointer. The owner is retired
on end, cancellation, or window blur; orphan move/end events are ignored.
This is touch-to-pointer routing, not a multi-finger pinch/zoom recognizer.

Both native hosts share `native_clipboard` for command-key C/X/V. Copy preserves
selection; cut deletes only after a successful clipboard write; a failed paste
read leaves text intact. Empty selection does not overwrite the clipboard.
Paste goes through the normal Runtime editor, including newline normalization
and binding synchronization. Platform adapters are synchronous and require the
platform clipboard tools already used by the standalone host. Automated tests
inject a clipboard and never read or replace the user's system clipboard.

Simple text and shaped metrics use bundled Ubuntu/NotoSansSC/NotoEmoji faces.
`text::ShapedGlyph::font_index` identifies0/1/2; `text::font_face(index)` returns
the immutable matching face (fallback weight400). A real fallback glyph has
its face-local glyph_id; glyph_id0 denotes synthetic unsupported text. Rust
callers constructing ShapedGlyph must initialize the new font_index field.
Colour/ZWJ emoji and full shaping are not qualified by scalar-font tests.
The crates/forma generated WASM with fonts is21,828,894bytes, gzip13,246,694bytes;
startup and deployment-size optimisation remain separate work.


### Property snapshot ownership

`form_document::Properties` uses shared immutable map storage with copy-on-write
mutation. Cloned snapshots retain value semantics; editing a map or nested object
detaches it. Public scene JSON remains unchanged. Rust callers that previously
passed a `BTreeMap<String, Value>` directly should use `.into()` or construct
`Properties::from([...])`. Map methods and iterator collection remain available.
Ordinary model maps stay flat COW. Evaluation scopes use immutable shared bases
and small copy-on-write deltas for For/Let and captured locals; small scopes
retain a bounded flat-copy path. Explicit whole-map views materialize entries;
owned list values and evaluation/export can still copy data. Borrowed iteration
returns `PropertyIter`: replace explicit `btree_map::Iter` annotations with that
type or a generic iterator. See `packages/svet/SNAPSHOT_STORAGE.md` for gates.


### Reusing RGBA output storage

Rust callers rendering repeated CPU frames can retain their output allocation:

```rust,ignore
let mut rgba = vec![0; width as usize * height as usize * 4];
runtime.paint_rgba(&mut rgba, width, height, scale, true)?;
```

`Runtime` and `Button` expose this API. The last argument selects background
(`true`) or content-only (`false`). Dimensions must exactly match the slice;
invalid sizes/DPI return an error without changing it. Warmed unchanged frames
allocate no output storage. Pixels are copied from the raster cache; initial
raster building and changed content may still allocate. Existing `pixels` and
`content_pixels` continue returning owned vectors for snapshot callers.

### CPU allocator lifecycle probe

```sh
cargo run --release --example binding_sync_bench -- --cpu-lifecycle > lifecycle.json
python3 scripts/qualify-cpu-lifecycle.py lifecycle.json lifecycle-summary.json
```

The probe runs both document schedulers at 8/32/96/256 controls. Each lifecycle
alternates 32 first/last binding edits and rebuilds CPU caches every fourth edit,
using retained RGBA/XRGB destinations. It checks exact models, unchanged rows,
RGBA and XRGB conversion. After one warm lifecycle, three measured lifecycles
record per-edit live allocator bytes, peak bytes and the delta after Drop.
Diagnostics are excluded from the tracked heap. These are single-thread Rust
allocator Layout bytes, not process RSS or GPU memory. A short synthetic series
does not qualify native input latency or long-duration leak freedom.

### Native GPU surface ownership

`native_gpu::GpuWindow` shares the surface/presentation lifecycle between the
standalone executable and reusable NativeHost. Desktop Svet requests GPU
presentation; setup/render failures use CPU fallback, and surface retries are
scheduled through the event loop. Mobile/chooser and egui-compatible profile
use CPU presentation. NativeApp may observe successful presents through optional
`on_frame_presented`; application scheduling remains separately owned. The
legacy egui profile is compatibility styling, not an integrated egui engine.
macOS Gpu→Cpu→Gpu and binding edits were observed; fault injection, OS resize/
occlusion/retry and cross-platform qualification remain open.


Native GPU device-loss notifications are serviced by `Device::poll`. Current
Metal implementation observes command completion AccessRevoked/DeviceRemoved,
retains the first cause and delivers a registered callback once (including late
registration). Renderer health polls before surface acquisition. A real Metal
Device with injected backend state qualifies delivery/reentry; physical device
removal/reset and idle removal notifications remain unqualified.


For native GPU-loss diagnostics, build the input probe with
`--features native-test-hooks --example native_input_probe`. F8 injects a
synthetic backend loss; the real host should present on CPU and preserve input.
An explicit F7 profile roundtrip requests a fresh GPU. `NativeApp::on_gpu_fallback`
can observe the reason. Ordinary `--features native` excludes these injection
hooks; keep `native-test-hooks` out of release feature sets. The macOS scenario
was observed through actual presentation journals and binding edits; physical
device reset/removal and automatic recovery remain separate qualification.


NativeHost CPU self-animation targets60Hz through event-loop deadlines;
input-driven redraws remain immediate. Slow frames are not delayed by another
full interval. GPU presentation retains surface FIFO pacing. Surface retry and
animation deadlines share the nearest wakeup;occluded windows cancel both.


Sparse CPU leaves now allocate coverage/content/clip scratch for a conservative
physical support region. Text/shape contours,clip arithmetic and reveal/spans
retain global coordinates;physical storage indexing is local. Dense/single
control output remains unchanged. Native and WASM exact-pixel comparisons
passed;paired release2x caret fixture requests fell8.70→1.12MB/frame.
See packages/svet/benchmarks/region-raster-qualification-2026-11-03.json for
raw timing,allocation budget and remaining dense/overflow/window scope.


CPU content scratch uses a conservative text/shape support region independently
of dense/sparse geometry storage. Nested clips share that region and retain
original global sample coordinates. Base coverage uses row spans; output
remains dense. Disjoint overflow may still require a large content union. See
[qualification](../packages/svet/benchmarks/dense-content-qualification-2026-11-03.json)
for exact-frame/allocation evidence and the remaining window/RSS limits.

Dense CPU caches also pack base coverage into global row spans. Their paint
passes process base/edges/content/reveal before dense frame/background
composition; RGBA/XRGB buffers remain dense and Frame metadata uses runs. Public content_pixels release
pair measurements and exact animated/clipped/scrolling WASM comparisons are in
[qualification](../packages/svet/benchmarks/packed-dense-qualification-2026-11-03.json).
These are not OS input latency or NativeHost/RSS results.


For manual large-form OS qualification, `native_form_probe` uses synthetic
8/32/96/256 bound mixed-font editors and the retained Svet scheduler. Choose
`FORMA_NATIVE_FORM_CONTROLS` and a fresh `FORMA_NATIVE_FORM_LOG` file before
running `cargo run --release --features native --example native_form_probe`.
The log path is overwritten on launch; preserve previous runs first. F7 switches
GPU/CPU presentation. Edit first/last rows and scroll both ways; the journal
records actual present callbacks and process peak RSS. Journaling/getrusage add
overhead; these callbacks do not measure GPU completion or OS input latency.
[macOS evidence and limits](../packages/svet/benchmarks/native-large-qualification-2026-11-03.json)
and `scripts/qualify-native-form.py` retain the supported verification scope.


Frame metadata now uses equal-value coverage/viewport runs. Original rounded
clip and scrollbar samples are preserved; homogeneous runs can skip pixel work.
Runs use exact monotone search across viewport/scrollbar intervals; scalar
fallback handles small/degenerate inputs. RGBA/XRGB output stays dense.
[Qualification](../packages/svet/benchmarks/frame-runs-qualification-2026-11-03.json)
includes background/content timing separately: content-only A/A is noisy and
remains exploratory. Exact WASM,hardware and updated256-editor OS evidence
cover the stated behavior; long lifecycle/RSS and other platforms remain open.


Frame run construction now searches monotone rounded-frame coverage halves
between exact pixel-center viewport/scrollbar boundaries. Contour rasterization
is unchanged. Small/degenerate/nonfinite Frame inputs retain scalar sampling.
[Scalar/pixel/performance qualification](../packages/svet/benchmarks/frame-spans-qualification-2026-11-03.json)
includes the24k-vs1.92M sample budget,paired cold-build timings and current
256-editor native behavior. OS latency/RSS,other platforms and long lifecycle
qualification remain separate.

### Grapheme-aware UI editing

Native/WASM UI editors move, select, hit-test and delete extended grapheme
clusters using unicode-segmentation1.13.3. Combining marks, emoji modifiers,
ZWJ sequences and flags remain atomic editing units. Insertion and live binding
replacement align the resulting caret to a cluster boundary. TextEdit retains
its scalar APIs and byte offsets; explicit move_graphemes/insert_graphemes/
backspace_grapheme/delete_grapheme methods serve grapheme-aware callers. Partial
scalar selections expand to whole clusters for these replacement operations.
This does not add full font shaping, BiDi or color/ZWJ emoji rendering.

### Logical word navigation

TextEdit::move_words/backspace_word/delete_word use non-whitespace UAX29
word-boundary segments. Punctuation and emoji are stops; Right targets segment
ends, Left targets starts, with grapheme-safe rounding. Word deletion is one
undoable operation. Native hosts map Option on macOS and Ctrl without Alt on
Windows/Linux. Runtime::text_word_key(key, shift) exposes the normalized action
to WASM hosts; browser DOM adapters must forward the platform modifier. Existing
text_key remains the plain grapheme/command path. This policy is not dictionary
segmentation or a claim of identical native behavior in every locale/platform.


TextInput accepts `label` for a stable accessible name and `readOnly: true` for
a focusable/selectable field that refuses user edits. Programmatic binding value
updates remain available. Readonly disables native IME admission; it differs
from disabled controls, which cannot receive editing focus. Browser and native
semantic exports expose readonly state. Cross-platform screen-reader and OS IME
qualification remains in the Svet plan.


Accessibility snapshots export `multiline`, `readOnly`, and `selectionUtf16`
(anchor/caret) for text controls. Snapshot offsets refer to the original node
value, including CRLF. The browser `text_selection_utf16` API instead refers to
textarea-normalized LF text; these coordinate systems must not be mixed.
