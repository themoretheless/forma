# Статус выполнения

| Этап | Состояние |
|---|---|
| 0. Базовая линия | частично: гейт, тесты крейтов, размеры, дубли, циклы и анализ снимков сняты; clippy, покрытие и бенчмарки нет |
| 1. Фундамент | готово, кроме ESLint и строгих флагов tsconfig для `packages/svet` (там параллельно работает другая сессия) и clippy-ratchet в CI (только локально на macOS) |
| 2. forma | начат: golden на все builtins, исправлены F-1 (unsafe в `Form::sync`) и F-2 (паника на цвете), диспетчер builtins разбит на 9 модулей, `WidgetState` вместо `Runtime` в документе, единый `blend`, модуль `geometry`; архитектурный тест с 2 известными нарушениями (`display_list`/`raster_cache` → `Button` в корне) |
| 3–8 | не начаты (в 6 сделан первый шаг: `raster-core` без лишних `clone`) |

Ниже утверждённый план без изменений; пути `vector-ui/...` в нём читать как `crates/forma/...`.

---

# Полный рефакторинг Forma: план

## Контекст

Запрос: полный рефакторинг репозитория `forma` (SOLID, DRY, clean code, паттерны, модульность,
структура проекта, system design, агрессивная оптимизация, проверка каждого модуля и метода).

Разведка и проектирование (шесть агентов, только чтение кода, ничего не собиралось и не запускалось)
показали около 300 тыс. строк и такие узкие места:

- **`vector-ui`** (пакет `forma`, 36 тыс. строк Rust): god-файлы `form_document.rs` (3567) и `runtime.rs` (3239),
  циклы модулей, два оконных хоста (`main.rs::App` и `native_window::NativeHost`), дублированная
  компоновка кадра в `Button` и `Runtime`, проход «документ → текст `.ui` → повторный разбор» на каждой
  пересборке, `Result<_, String>` в 208 сигнатурах, ноль прямых тестов у `eval.rs`/`bindings.rs`/`compiler.rs`.
- **`packages/svet`**: `compileFormaComponent` на 1437 строк, `gpu.ts` на 2301 строку с шестью
  ответственностями, `svet-wasm/src/lib.rs` на 2540 строк.
- **Studio** (`src/*.js`): `main.js` с 37 импортами и ~20 модульными `let`, `vite.config.js` как dev-бэкенд,
  таблица MCP-инструментов в двух местах.
- **`crates/*`**: продукт использует три крейта из десяти; `gpu-runtime` содержит 111 `transmute(objc_msgSend)`
  и ноль `// SAFETY:`; у `gpu-shader` нет тестов; CI не исполняет Metal-путь и не собирает крейты отдельно.
- **Гейты:** нет форматтера, линтеров, `cargo deny`, корневого workspace.

Объём работы около 450–500 атомарных коммитов, это много сессий. Поэтому бэклог и статус живут в
репозитории (`docs/refactor/`), а этот файл задаёт архитектуру, порядок и правила.

## Решения владельца

| Вопрос | Решение |
|---|---|
| Границы | Всё, включая семь неиспользуемых крейтов; они идут полным проходом после продукта |
| Контракты | Ломать можно; каждое изменение в `docs/refactor/MIGRATIONS.md` |
| Форматирование | Отдельными чисто форматирующими коммитами в начале |
| Git | Коммиты прямо в `main`, один на атомарный шаг, без пуша |
| Каталог `vector-ui` | Переименовать в `crates/forma` |
| Замороженные снимки `*-source` | Сначала анализ «влито или нет»; удалять только влитое |
| Заглушки бэкендов | Реализовывать (отдельный трек после рефакторинга, этап 8) |

Принято по рекомендациям проектирования (можно оспорить при утверждении):

- `forma` остаётся одним крейтом с каталогами по слоям и архитектурным тестом; на крейты не дробится.
- Studio остаётся на JS с `// @ts-check` + JSDoc и в корне репозитория (корень служит корнем Vite).
- Ядро реактивности `svet` не дробится на файлы; декомпозиция внутри модуля, разбиение только как замеренный эксперимент.
- ABI генераторов не ломается: `forma::Runtime`, `forma::binding::*`, `forma::form_document::*`,
  `forma::native_window::{run, NativeWindowConfig, NativeApp}`, `forma::mcp::*`, включая `Result<_, String>` на границе.
- Не меняются: имена MCP-инструментов, ключи хранилища, события `forma:*`, `FORMA-TEMPLATES-1`,
  `forma-scene-artifact/1`, имена wasm-экспортов `Runtime`, JNI-символы, тексты ошибок, путь `vector.wgsl` внутри крейта.
- Псевдонимы `wgpu`/`naga` сохраняются: на Linux `forma` собирается с настоящим `wgpu 29`, поверхность API общая.
- Ошибки: типизированные enum с ручным `Display` и `source()` (у листовых крейтов ноль зависимостей), без `thiserror`.
- `native-app` исключается из workspace (нужен `FORMA_ACTIONS` и GTK); `svet-wasm` входит с именованным профилем.

## Инварианты выполнения

1. Поведение сохраняется, кроме изменений из `MIGRATIONS.md`.
2. Тесты зелёные после каждого шага; для непокрытой логики сначала характеризационные тесты (golden-файлы без новых зависимостей, `FORMA_BLESS=1` для обновления).
3. Перемещения отдельно от логики, форматирование отдельно от всего. Переносить по имени элемента: номера строк в отчётах до форматирования.
4. Оптимизация только с замером до/после существующим бенчмарком; в пределах шума изменение отклоняется.
5. Найденный баг: запись в `docs/refactor/BUGS.md`, затем отдельный коммит с падающим тестом и исправлением.
6. Предупреждения исправляются; `allow` не добавляется. Счётчики линтов только убывают (ratchet).
7. После каждого Rust-коммита в `crates/forma/src` пересобирать wasm (`npm run build:wasm`): дайджест исходников входит в гейт.
8. `compute-cuda`: без изменения порядка вызовов драйвера и `.cu`; в сообщении коммита «на CUDA-железе не запускалось».
9. Коммиты: обычное предложение, без префиксов типа и без упоминаний ассистента.

## Целевая архитектура

### Репозиторий

```
Cargo.toml            корневой workspace: общий lock, target, [workspace.lints], профили release и svet-wasm
crates/forma/         бывший vector-ui
crates/{gpu-shader,gpu-runtime,gpu-compute,raster-core,math-core,math-compute,
        tensor-core,compute-core,compute-mlx,compute-cuda}/
packages/svet/        TS-движок + crates/svet-wasm
src/ test/ scripts/   Studio (корень Vite), папки по фичам
protocol/             enums, имена событий, описание моста, единая таблица MCP-инструментов
dev-server/           плагин Vite: workspace API, каналы, MCP-мост, runners/*
mcp/                  server, schema (таблица → zod), bridge-client
tools/                живые скрипты квалификации (NVRTC)
docs/refactor/        BASELINE, INVENTORY, ARCHITECTURE, PLAN, MIGRATIONS, BUGS, lint-baseline.json
docs/adr/             решения
```

Слои крейтов (проверяет `scripts/check-layering.mjs` по `cargo metadata`):
`gpu-shader`, `tensor-core`, `math-core`, `svet-wasm` без внутренних зависимостей → `gpu-runtime` → `gpu-compute`
→ `raster-core`, `compute-core`, `compute-cuda`; `compute-mlx` → `tensor-core`; `math-compute` поверх всех вычислительных;
`forma` → `gpu-runtime`, `svet-wasm` и никогда вычислительные крейты.

### `crates/forma/src` (зависимости только вниз, `pub(crate)` по умолчанию)

| Слой | Модули | Откуда |
|---|---|---|
| L0 `foundation` | `geometry`, `blend` (один `blend`, `over` как обёртка), `color` (один парсер), `hash`, `error` | `lib.rs`, `text.rs`, `raster_cache.rs`, `template.rs`, `eval.rs`, `mcp.rs` |
| L1 `text` | `unicode`, `fonts`, `shaping`, `metrics`, `wrap`, `outline` | `text.rs` |
| L1 `models` | по файлу на модель + `error` | `control_state.rs` |
| L1 `language` | `markup`, `transport`, `template/{model,lexer,parser,builder,cache}` | `template.rs`, куски `runtime.rs`/`form_document.rs` |
| L2 `document` | `model`, `value`, `kind` (`NodeKind`), `properties`, `contracts`, `svg`, `eval/{path,operators,member,pattern,builtins/*}`, `compiler/*`, `expand/{control_flow,visual,instantiate,scene,…}`, `layout/{grid,…}`, `emit/{flatten,scene_text,typed}`, `bindings/*`, `artifact`, `widget_state` | `form_document*` |
| L2 `render` | `scene`, `display_list/*`, `reveal`, `cpu/{coverage,text,shape,frame_mask,control_cache}` | `display_list.rs`, `raster_cache.rs`, `text.rs`, `shape.rs` |
| L3 `controls` | `control`, `control_raster`, `control_vector`, `editor`, `animation`, `reveal_state` | `lib.rs::Button`, `text_input.rs` |
| L4 `runtime`, `binding` | `runtime/{loading,a11y,focus_modal,editors,raster,vector,ui_bindings,api_*}`; `binding/{property,context,field,control,form/{sync,update}}` | `runtime.rs`, `binding.rs` |
| L5 хосты | `gpu/*`, `native/{app,host/*,dev_app,platform,…}`, `mcp/*`, `wasm/*`, `android_jni`, `demo_ui`; `main.rs` только CLI | `gpu.rs`, `native_*.rs`, `main.rs`, `mcp.rs` |

Ключевые развязки: `text` ниже `document`; `document` не знает `Runtime` (трейт `WidgetState`);
`render` принимает `ControlPaint` и не видит `Control`; `App` становится `DevApp: NativeApp` на едином `NativeHost`;
типизированный `SceneIr` от документа к `Runtime`, текстовый вход остаётся вторым фронтендом для Studio.
Старые публичные пути живут через `pub use` до финального шага. Порядок enforced `tests/architecture.rs`
(сначала ratchet со списком текущих нарушений, в конце строгий).

### `packages/svet/src/engine`

Текущие файлы верхнего уровня остаются фасадами-реэкспортами (на них завязаны 21 тест и скрипты).
Каталоги: `reactivity/{kernel (горячий путь целиком), store, captured-context, isolation, telemetry, errors, types}`,
`dom/{events, model/*, props, style, attrs, insert, h, show, for, mount}`,
`gpu/{state, configure, tensor, arena-sync, compute/*, scene3d/*, widgets/sparkline, resident/*}`,
`forma/{parse/*, ast/*, validate, aot, expr/*, stdlib, instance/*, render/{context,node,element,tags,props,handlers/*}}`,
`async/*`, `router/*`, `reactive-extras/*`. Слои L0 wasm → L1 reactivity → L2 dom → L3 async/router → L4 gpu → L5 forma → L6 ssr → L7 barrel;
`forma/` импортирует `gpu/` только из `render/handlers/scene3d.ts`; `compiler.ts` импортирует только `forma/aot.ts`.
`svet-wasm`: `graph/*`, `lis`, `gpu/{arena,math,clip,pick}`, `context` (`Core { graph, scratch, gpu }`), `ffi/*`.

### Studio `src/`

`main.js` только композиция. Слои: `shared` → `language` (единственный импортёр токенизатора) →
`compiler`, `codegen`, `edit` (Node-safe) → `app` (`state.js`, `studio.js`) → фичи (`workspace`, `editor`, `preview`,
`canvas`, `inspector`, `tree`, `output`, `shell`), которые общаются только через `app` → `bridge`.
Фасады `src/{components,language,vector-preview}.js` остаются для внешних импортёров.
Правила проверяет расширенный `test/module-graph.test.js`.

## Бэклог по этапам

Каждый этап заканчивается полным гейтом и обновлением `docs/refactor/*`.

### Этап 0. Базовая линия (2–3 коммита, только документы)
- Тег `refactor-base`; `npm run build:wasm`, `GATE_VERBOSE=1 npm run gate`, svet-шаги из `ci.yml`.
- Сборка и тесты всех десяти крейтов с `COMPUTE_REQUIRE_GPU=1`/`COMPUTE_REQUIRE_MLX=1` (без них тесты проходят вхолостую); CUDA: «скомпилировано, хост-тесты N, устройство недоступно».
- Метрики: clippy по линтам (`--force-warn` сквозь `allow`), длина и сложность каждой функции, LOC, `jscpd`, `cargo udeps`, `knip`, `madge`, покрытие, размеры wasm, время сборки, `npm run bench`, бенч-примеры.
- Анализ снимков `*-source`: для каждого каталога diff против живого `src/` и истории; таблица «идентичен / живой код новее / в снимке есть невлитое» в `docs/refactor/SNAPSHOTS.md`.
- Результат: `BASELINE.md`, `INVENTORY.md` (каждый модуль и функция с находками), `ARCHITECTURE.md`, `PLAN.md`, `BUGS.md` с уже найденными дефектами.

### Этап 1. Фундамент (около 35 коммитов)
1. Тесты, сверяющие текст исходников, сделать устойчивыми к формату (`test/{mcp-commands,launch-navigation,vector-preview,vector-gpu}.test.js`).
2. `rust-toolchain.toml` + `components = ["rustfmt","clippy"]`; `rustfmt.toml` (`style_edition = "2024"`, ширина 100), `.editorconfig`, Prettier (ширина 100, `embeddedLanguageFormatting: off`, игнор `wasm-binary.ts`, evidence, `.md`, `.ui`, `.html`).
3. Форматирующие коммиты по одному на подпроект: крейты, `vector-ui` + примеры, `svet-wasm` вместе с перегенерированным `wasm-binary.ts`, `native-app`, Studio JS, svet TS. Затем `.git-blame-ignore-revs`.
4. Корневой `Cargo.toml` (lock посеять из `vector-ui/Cargo.lock`), правки `scripts/{versions,tokenizer,runtime-digest,build-wasm,perf-bench,window-bench}.mjs`, двух `.sh`, `ci.yml`, тестов `versions`/`tokenizer`/`full-gate`, `.gitignore`.
5. `git mv vector-ui crates/forma` одним коммитом с правкой ~75 файлов (манифесты, `build.rs` примеров, 16 скриптов, 11 файлов `src/`, 14 тестов, 8 документов, `vite.config.js`, `.mcp.json`); запись в `MIGRATIONS.md` (URL галереи, путь по умолчанию в native-экспорте).
6. Удаление снимков, признанных влитыми по `SNAPSHOTS.md`, после тега; живые NVRTC-скрипты в `tools/`; правка ссылок в README крейтов.
7. Гейты: `cargo fmt --check`, `scripts/clippy-ratchet.mjs` + `lint-baseline.json`, `deny.toml`, `scripts/check-layering.mjs`, ESLint (flat config, ratchet через suppressions), `tsc` для JS, svet tsconfig-флаги по одному; всё в `scripts/full-gate.mjs` и `ci.yml`.
8. Крейты в гейте и CI (job `crates` на трёх ОС, Metal-шаги на macOS), проверки `wasm32`, Android и iOS для `forma`.
9. Архитектурные тесты в режиме ratchet: `crates/forma/tests/architecture.rs`, `packages/svet/tests/module-graph.test.ts`, матрица слоёв в `test/module-graph.test.js`.

### Этап 2. `forma` (около 110 коммитов)
- **A. Характеризация (18):** golden на каждый builtin `eval::call` и на `eval_mode`; контракты, раскладка, SVG; golden компилятора по `controls/components/*.ui`; golden полного вывода `render_scene_with_extents`; `over ≡ blend(…,1.0)`; `Button` против одноконтрольного `Runtime`; `Form::sync/update`; MCP и a11y JSON.
- **B. Перемещения (≈25):** по таблице слоёв, один коммит на группу; разрывают циклы `text↔shape`, `template↔text_input`, родитель↔дети в `document`.
- **C. Логика (≈30):** один `blend` и один парсер цвета; именованные константы; типизированные ошибки с тем же `Display` (убрать `starts_with` по тексту ошибки); `NodeKind`; `eval::call` → семейства builtins за одной диспетчеризацией; общие селекторы control-flow и сеточный placer для двух уровней раскрытия; `render_scene_with_extents` по фазам; `ContentSink`; `TemplateBuilder`; `SceneIr` и `Runtime::from_scene_ir`; `WidgetState`; безопасный обход вместо `unsafe` в `binding.rs` и `DocumentTxn` для отката; RLE-маска кадра в `Runtime`; `Control` + вид кадра, удаление wasm-класса `Button`; `ImeSession`, `InputMapper`, `DevApp` и удаление `App`; одна таблица MCP-инструментов; строгий архитектурный тест и сужение публичного API.
- **D. Оптимизации по замеру:** типизированная передача сцены (`binding_sync_bench`), переиспользование `Arc<Template>` и растрового кэша, аллокации в `Form::update` и в `For`, `NodeKind` вместо `String`, затем дорожная карта `PERFORMANCE.md` (флаги layout/paint, dirty rects, кэш глифов).

### Этап 3. `svet` (около 70 коммитов)
- Характеризация: снимок имён экспортов каждой точки входа, golden рендерера по семействам тегов, корпус парсера и stdlib, тест ABI (`SvetWasmExports` = `WASM_EXPORTS`), таблица ошибок async-диспетчера.
- Перемещения от безопасных к рискованным: `forma-reactive`, `advanced`, `devtools`, не-инстансная часть `forma.ts`, `expr`/`stdlib`, `gpu`, `dom`, холодные части `reactivity`; Rust: тесты, `impl` по модулям, `ffi`, `context`.
- Логика: `compileFormaComponent` → `FormaInstanceCore` из фабрик + `RenderContext` + таблица тегов и реестр обработчиков; `createCanvas3D` → пять коллабораторов; линейный конвейер async-вычислений; общие помощники в `signal`/`computed` внутри ядра; `RowStore` + `planReconcile` в `For`; таблица стратегий в `bindModel`; роутер, ресурс, форма; `Core { graph, scratch, gpu }`; удаление 10 неиспользуемых wasm-экспортов; компилятор убрать из корневого barrel; `any` → `unknown` и размеченные объединения.
- Оптимизации: аллокация замыкания на чтение сигнала, `frames.at(-1)`, порядок инициализации полей сигнала, `Map` вместо цепочки сравнений тегов, быстрые пути `For`; мера `bench-reactive-lifecycle.mjs`, `bench-forma-snapshot.mjs`, парные установки.

### Этап 4. Studio (около 60 коммитов)
- Характеризация: golden компилятора (`compile-digest` как тест + негативный корпус на 88 `throw`), golden `buildNativeRustExport`, все 45 MCP-команд на фейковом `studio`, dev-сервер (обход путей, лимиты, гварды), чистые куски `main.js`.
- Перемещения: `shared/`, `language/`, `edit/`, `codegen/`, `project/`, фичи; `components.js` → `compiler/*`; runners и плагин → `dev-server/`.
- Логика: единый объект состояния → `app/state.js`; сервисы с инъекцией портов; workspace на командах с `undo`; `.onX=` → `scope.listen` вместе с переводом MCP-вызовов на сервисы; `protocol/mcp-tools.js` как единственная таблица; `compile()` → `CompileContext` и пять фаз; разбиение `native-export` и `vector-preview`; `// @ts-check` послойно; разбор `check-package.mjs` на фазы и фикстуры.
- Оптимизации: клоны в кэше компилятора, сеточные префиксные суммы, делегирование событий дерева; мера `compiler-bench.mjs`, `compiler-session-bench.mjs`.
- Тест соответствия двух реализаций языка (svet и токенизатор) с проверяемым манифестом расхождений.

### Этап 5. `gpu-shader`, `gpu-runtime`, `gpu-compute` (около 90 коммитов)
- Сначала страховка: golden WGSL→MSL по всем шейдерам репозитория, проверка компиляции каждого golden на Metal, тест поверхности `wgpu::`, поведенческие Metal-тесты, прогон вычислительных крейтов на Metal как harness.
- `gpu-shader`: `arena`/`ir`, `front/wgsl/*`, `back/msl/*`, общий `text`; `BodyCtx`, диспетчеры по конструкциям и таблицы вызовов, один разбор атрибутов, типизированная ошибка с реальной позицией, `validate_wgsl`.
- `gpu-runtime`: `metal/{ffi,selectors,device,buffer_pool,pools,*_pipeline,texture,sampler,queue,submit,surface}`, `command/*` отдельно от Metal-кодирования, тесты из `lib.rs`; типизированные обёртки `msg_*` вместо 111 `transmute`; декомпозиция `submit_with_error_sink`, `create_render_pipeline`, `encode_render_pass`; снятие crate-wide `allow` помодульно с `// SAFETY:`; политика resize `CAMetalLayer` из `forma` сюда.
- `gpu-compute`: `buffer_entry` с параметром видимости, `source()` у ошибок.

### Этап 6. Семь вычислительных крейтов (около 100 коммитов)
Порядок: `tensor-core`, `compute-core`, `raster-core`, `math-core`, `math-compute`, `compute-mlx`, `compute-cuda`.
Разбиение `host_tensor.rs`, `spatial.rs`, `postprocess.rs` по семействам; `RasterPipelines::new` (426 строк) → таблица `PipelineSpec`;
общий `tests/support` в `compute-mlx`; `// SAFETY:` по шаблонам FFI; `expect`/`unwrap` в продакшен-коде → типы;
двойной разбор WGSL в `compute-core` как замеряемая оптимизация; переименование пакетов `osv-math*` в `math-core`/`math-compute`.

### Этап 7. Чистка и финал
Мёртвый код и зависимости, битые ссылки `../../docs` в README крейтов, устаревшие `ARCHITECTURE_STEPS.md`/`PLAN.md`,
таблица до/после в `BASELINE.md`, остаточный бэклог, все строки `INVENTORY.md` в статусе done или «отложено» с причиной.

### Этап 8. Реализация бэкендов-заглушек (новая функциональность, не рефакторинг)
Идёт после этапа 5, каждый бэкенд со своим ADR и тестовой матрицей. Порядок по проверяемости:
1. SPIR-V в `gpu-shader` + Vulkan в `gpu-runtime`: проверяется в CI на Linux (lavapipe) и теми же golden/поведенческими тестами.
2. HLSL + DX12: проверяется только на Windows-раннере CI.
3. GLSL + OpenGL, затем WebGPU (wasm).
4. CUDA и RTX: на этой машине не проверяются; реализация только при наличии NVIDIA-хоста, иначе остаются честно помеченными как нереализованные.
До реализации заглушки перестают возвращать выдуманные адаптеры (это дефекты из `BUGS.md`).

## Изменения контрактов (в `MIGRATIONS.md`)

`vector-ui` → `crates/forma` (пути, URL галереи контролов, путь по умолчанию в экспорте); общий `Cargo.lock` и `target/`;
удаление wasm-класса `Button` (тесты и `vector.html` переходят на `Runtime`); `Node.kind: NodeKind`;
поведение dev-бинарника после слияния хостов (touch, клавиша F9 тестового хука, F10 выносится в `native/aux_window.rs`);
сужение публичного Rust API; компилятор только из `svet-engine/compiler`; минус 10 wasm-экспортов `svet`;
пути модулей Studio и runners; форма фасада `createIdeCommands`; пакеты `math-core`/`math-compute`; профиль `svet-wasm`.

## Уже найденные дефекты (чинятся отдельно, каждый с тестом)

`src/main.js:138` `persist()` сообщает «сохранено» при ошибке записи; `binding.rs:1204-1221` выдаёт пересекающиеся `&mut`;
`eval.rs:356-381` режет строку по байтам (паника на не-ASCII); три разных правила для детей `let`; расхождение прокрутки `Button`/`Runtime`;
потеря движения при медленном touch в `main.rs`; `TwoButtons.ui` не входит в дайджест wasm; CI не компилирует Android/iOS;
`gpu-runtime`: `objc_msgSend` с возвратом `CGRect` на x86_64, проглоченные ошибки шейдера с последующим `expect`, кэш пайплайнов по одному 64-битному хэшу;
`gpu-shader`: вычисление констант без вычитания, неизвестные типы как `u32`, позиция ошибки всегда 1:1; `mcp/smoke.js` ждёт 34 инструмента из 45.
Все они получены чтением кода и требуют подтверждения падающим тестом.

## Проверка

После каждого Rust-коммита в `forma`:
`cargo fmt --check`; `cargo test --locked -p forma --features gpu --lib --tests`; `cargo test --locked -p forma --features native --lib --bin forma`;
`cargo check --locked -p forma --features native --all-targets`; `cargo check --locked -p forma --target wasm32-unknown-unknown --lib`.

В конце группы шагов: тесты `binding-app`, запуск `language-app`, `npm run build:wasm`, проверки
`check-scene-{artifact,snapshot}-wasm.cjs`, `compare-*-wasm.cjs` против базового wasm, `npm test`, `npm run build`.

`svet` (из `packages/svet`): `npx tsc --noEmit`, `npm test`, `npm run test:actions`, `cargo test` для `svet-wasm`,
`npm run build:wasm` и сверка `wasm-binary.ts`, в конце этапа `npm run test:package`.

Крейты: `COMPUTE_REQUIRE_GPU=1 COMPUTE_REQUIRE_MLX=1 cargo test --locked -p …`; Metal: `cargo test -p gpu-runtime -- --include-ignored`.

Конец этапа: `npm run gate` и таблица замеров в `BASELINE.md`. Слияние хостов дополнительно вручную:
`scripts/run-ios-simulator.sh`, `scripts/run-android-emulator.sh`, `qualify-native-*.py`, запуск Studio (`npm run dev`) и `node mcp/smoke.js`.

Что проверить нельзя: устройство CUDA; DX12 локально; поведение на реальных iOS/Android-устройствах.

## Первая сессия после утверждения

Этап 0 целиком, затем этап 1 в указанном порядке. К логике (этапы 2+) не переходить, пока гейт с новыми
шагами не зелёный и `INVENTORY.md` не заполнен.
