# Рефакторинг Forma: правила и статус

Открытая работа и её порядок ведутся в [едином плане развития](../ROADMAP.md).
Здесь остаются статус этапов и правила, по которым рефакторинг выполняется.
Полный текст утверждённого плана с бэклогом по этапам и дополнением архитектурного ревью
сохранён в [PLAN_2026_11_12.md](PLAN_2026_11_12.md).

## Статус выполнения

| Этап | Состояние |
|---|---|
| 0. Базовая линия | частично: гейт, тесты крейтов, размеры, дубли, циклы и анализ снимков сняты; clippy, покрытие и бенчмарки нет |
| 1. Фундамент | готово: формат, ESLint (корень и svet), clippy, `cargo deny`, слои крейтов, workspace, крейты/Android/iOS/wasm в гейте, архитектурные тесты; строгий tsconfig svet включён целиком, включая `noUncheckedIndexedAccess` (ratchet по файлам снят после нуля находок) |
| 2. forma | начат: golden на все builtins, исправлены F-1, F-2, F-6; builtins разбиты на 9 модулей; `WidgetState`; единый `blend`; модуль `geometry`; `form_document.rs` 4860 → 548 строк, `runtime.rs` 4404 → 1635, `control_state` разбит на 14 файлов, `binding.rs` без `ModelValue` и тестов, тесты всех крупных модулей вынесены в отдельные файлы; `render_scene_with_extents` разбит на фазы под golden полного вывода (397 → 115 строк в самой длинной функции); `scene_expand` разбит по видам узлов (392 → 118); `Form::sync` и `Form::update` разбиты (209 и 251 → 68 в самой длинной функции `binding.rs`); `Node.kind` стал `NodeKind`; `text.rs`, `template.rs`, `binding.rs`, `runtime.rs`, `native_window.rs` и привязки документа разнесены по подмодулям без изменений кода, `Button` вынесен из корня крейта в модуль `control` со своим слоем в архитектурном тесте; осталось: слияние оконных хостов |
| 3–4 | не начаты |
| 5. GPU-крейты | начат: golden WGSL→MSL по 123 шейдерам; `gpu-shader` разнесён на `front/wgsl/*` и `back/msl/*`, тихие подстановки заменены ошибками, фиктивные бэкенды удалены; в `gpu-runtime` `metal`, `command` и `types` разнесены по модулям, тесты вынесены из `lib.rs`, `objc_msgSend` типизируется в одном месте (2 `transmute` вместо 118), ошибки пайплайнов стали validation errors; осталось: `// SAFETY:` со снятием crate-wide `allow`, длинные функции кодирования, настоящая валидация шейдеров |
| 6 | начат: `host_tensor.rs`, `postprocess.rs` и `spatial.rs` разнесены по семействам, пайплайны `raster-core` описаны таблицей, пакеты `osv-math*` переименованы; осталось: `// SAFETY:` по FFI, `expect` / `unwrap` в продакшен-коде |
| 7. Чистка | начат: битые ссылки в README крейтов, явная зависимость `@codemirror/search`; `cargo udeps` чист (кроме намеренной зависимости шаблона `openscad-ui`) |
| 8. Бэкенды | начат первый шаг: пробы Vulkan, OpenGL, WebGPU, DX12 больше не выдумывают устройства; реализации бэкендов нет |

Пути `vector-ui/...` ниже читать как `crates/forma/...`.

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

## Изменения контрактов (в `MIGRATIONS.md`)

`vector-ui` → `crates/forma` (пути, URL галереи контролов, путь по умолчанию в экспорте); общий `Cargo.lock` и `target/`;
удаление wasm-класса `Button` (тесты и `vector.html` переходят на `Runtime`); `Node.kind: NodeKind`;
поведение dev-бинарника после слияния хостов (touch, клавиша F9 тестового хука, F10 выносится в `native/aux_window.rs`);
сужение публичного Rust API; компилятор только из `svet-engine/compiler`; минус 10 wasm-экспортов `svet`;
пути модулей Studio и runners; форма фасада `createIdeCommands`; пакеты `math-core`/`math-compute`; профиль `svet-wasm`.

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
