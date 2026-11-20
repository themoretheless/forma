# Целевая архитектура

Полное описание открытых архитектурных задач и порядка работ ведётся в
[едином плане развития](../ROADMAP.md), а пофайловый статус модулей — в
[инвентаризации](INVENTORY.md). Здесь фиксируется текущее соответствие целевой структуре.

## Границы продукта

- **Единственный рекомендуемый путь приложения на Forma:** декларативный UI на `.ui`, сборка
  типизированных форм в Rust через крейт `forma-build` (`build.rs`, автономно без Node.js) поверх
  типизированной `ViewModel` (`forma::binding::{Context, Property}`), исполнение на десктопе через
  `forma::native_window` (`NativeApp`) и в браузере через WASM-сборку `crates/forma`
  (`public/vector-pkg`). Legacy WebView-хост `native-app` (`tao`/`wry`) выведен из эксплуатации.
- **Роль `svet-engine` (`packages/svet`):** самостоятельный реактивный UI/DOM/WebGPU-движок; из
  корневого workspace с `forma` его связывает только чистый DAG-крейт `crates/svet-graph` без
  обратных зависимостей в браузерный рантайм.
- **Изоляция GPU и вычислительных/ML-крейтов:** `gpu-shader`, `gpu-runtime`, `gpu-compute`,
  `gpu-kit`, `raster-core`, `math-core`, `math-compute`, `tensor-core`, `compute-core`,
  `compute-mlx`, `compute-cuda` и `swap-store` отделены строгим ациклическим графом зависимостей
  (`scripts/check-layering.mjs`), так что `forma` не зависит от прикладных ML/CUDA/compute-крейтов.
- **Политика компилятора Rust:** минимально поддерживаемый стабильный компилятор по workspace —
  `rust-version = "1.92"`; точный воспроизводимый toolchain для WASM-артефактов и lint-ratchet в CI
  закреплён в `rust-toolchain.toml` (`1.97.1`).

## Сделано

- Каталог ядра называется так же, как пакет: `crates/forma`.
- Корневой Cargo workspace: один lock (`Cargo.lock`), один `target/`, общий профиль `release`, включая
  `gpu-compute`, `gpu-kit`, `forma-build` и примеры `binding-app`/`language-app`; вне workspace остаётся
  только самодостаточный npm-пакет `svet-wasm` (ADR 0001). Legacy хост `native-app` удалён.
- Строгий ациклический граф (DAG) на всех уровнях репозитория:
  - Все 21 Cargo-крейт (включая `forma-build`, `svet-graph`, `svet-reconcile`, `svet-gpu-arena`,
    `gpu-compute`, `gpu-kit`, `swap-store` и standalone-манифест `svet-wasm`) проверяются в
    `scripts/check-layering.mjs` и `test/check-layering.test.js` на отсутствие циклов и
    неразрешённых рёбер; `crates/forma` зависит только от `crates/svet-graph` внутри корневого
    workspace без обратных ссылок в `packages/svet/crates/svet-wasm`.
  - Внутри `crates/forma/src` все 27 модулей образуют строгий послойный DAG (слои `0..=10`, `layer(to) < layer(from)`, 0 циклов, `KNOWN_VIOLATIONS = &[]` в `crates/forma/tests/architecture.rs`); подсистемы `form_document/*`, `control_state/*`, `text/*` и `runtime/{a11y,canvas,links,modal,popup,raster,wasm_api}.rs` разнесены по модулям.
  - Графы импортов Studio JS (`protocol/`, `dev-server/`, `mcp/`, `src/`) и `packages/svet/src/**/*.ts` покрыты проверкой отсутствия циклов и соблюдения границ слоёв в `test/check-layering.test.js`.
- В `crates/forma` добавлены прикладной контрол `Canvas` (`canvas.rs`, `runtime/canvas.rs`), шрифтовые начертания `FontFamily::Mono` (`IBMPlexMono-Regular`) и `FontFamily::SansBold` (`Ubuntu-Bold`) со стилизованными текстовыми фрагментами и ссылками (`text/runs.rs`, `form_document/text_runs.rs`, `runtime/links.rs`), якорные немодальные попапы/меню/тултипы (`runtime/popup.rs`), приём внешних обновлений `StatePush` в `Runtime` и режим `HostMode::Application` с `NativeWaker` в `native_window.rs`.
- В `crates/gpu-kit` объединены GPU-примитивы (`backend`, `model`, `material_render`, `material_texture`, `exposure`, `histogram`, `hud`, `filmstrip`) поверх общего строителя render/compute пайплайнов и экранных uniform/vertex буферов (`pipeline.rs`) и `memory-budget`.
- Rust-ядро Svet разделено на три независимых крейта корневого Cargo workspace (`crates/svet-graph`, `crates/svet-reconcile`, `crates/svet-gpu-arena`) и тонкую C-ABI/WASM-оболочку `packages/svet/crates/svet-wasm` (`Core { graph, lis, gpu, scratch_in, scratch_out }`) с защитой от реентерабельного `&mut` и безопасной аллокацией `Box<[u32]>`; в `packages/svet` разделены движок Forma (`forma-ast`, `forma-aot`, `forma-dom`, `forma`) и 3D/графические модули GPU (`gpu`, `gpu-scene3d`, `gpu-sparkline`, `gpu-resident`) без взаимных реэкспортных циклов; ускорена пакетная очистка `For`, снижен baseline `typecheck:indexed` (741 → 621).
- Линтеры и контроль качества:
  - Корневой JavaScript: 0 подавлений в `eslint-suppressions.json` (`{}`), граф модулей проверяется в `test/module-graph.test.js` и `test/check-layering.test.js`.
  - Rust workspace Clippy ratchet (`scripts/clippy-ratchet.mjs`): **0 предупреждений во всём workspace** (`docs/refactor/lint-baseline.json = {}`).
- Серверная часть Studio вынесена из `vite.config.js` в модульный слой `dev-server/` (`wasm-middleware.js`, `mcp-bridge.js`, `runner-ws.js`, `index.js`), а `vite.config.js` содержит только декларативную конфигурацию сборки и dev-сервера.
- В `crates/forma/src/form_document` выделен быстрый контур синхронизации биндингов (`Document::sync_runtime_bindings`, -74% аллокаций на горячем пути) и атомарная транзакционная запись батчей в `binding.rs`.
- Протокольный слой `protocol/` (`enums.js`, `mcp-tools.json`, `mcp-tools.js`) и встроенный `forma::mcp` объединяются в едином Rust MCP-сервере `crates/forma-mcp` (57 инструментов: 45 Studio IDE bridge + 12 Forma Runtime UI, и 3 ресурса `forma://language`, `forma://ui/source`, `forma://ui/status`) вместе с `mcp/smoke.js` и доменными обработчиками `src/mcp-commands-{project,design,runtime}.js`.
- Один релизный тег `vX.Y.Z` на библиотеку и Studio; единый стиль rustfmt (style edition 2024, ширина 100) и Prettier (ширина 100).

## Не сделано

- Дальнейшая декомпозиция оконных хостов (`crates/forma/src/native_window.rs`, `crates/forma/src/main.rs`) и UI-оболочки `src/main.js`.
