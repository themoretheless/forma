# Целевая архитектура

Полное описание слоёв, деревьев модулей и правил зависимостей находится в `PLAN.md`, раздел «Целевая архитектура».
Здесь фиксируется только то, что уже соответствует цели, и то, что ещё нет.

## Сделано

- Каталог ядра называется так же, как пакет: `crates/forma`.
- Корневой Cargo workspace: один lock, один `target/`, общий профиль `release`; `gpu-compute`, `gpu-kit`, `svet-wasm` и `forma-native` вне его со своими lock-файлами (ADR 0001).
- Строгий ациклический граф (DAG) на всех уровнях репозитория:
  - Все 18 Cargo-крейтов (включая standalone-манифесты `gpu-compute`, `gpu-kit`, `svet-wasm`, `forma-native` и `swap-store`) проверяются в `scripts/check-layering.mjs` и `test/check-layering.test.js` на отсутствие циклов и неразрешённых рёбер.
  - Внутри `crates/forma/src` все 27 модулей образуют строгий послойный DAG (слои `0..=10`, `layer(to) < layer(from)`, 0 циклов, `KNOWN_VIOLATIONS = &[]` в `crates/forma/tests/architecture.rs`); подсистемы `form_document/*`, `control_state/*`, `text/*` и `runtime/{a11y,canvas,links,modal,popup,raster,wasm_api}.rs` разнесены по модулям.
  - Графы импортов Studio JS (`protocol/`, `dev-server/`, `mcp/`, `src/`) и `packages/svet/src/**/*.ts` покрыты проверкой отсутствия циклов и соблюдения границ слоёв в `test/check-layering.test.js`.
- В `crates/forma` добавлены прикладной контрол `Canvas` (`canvas.rs`, `runtime/canvas.rs`), шрифтовые начертания `FontFamily::Mono` (`IBMPlexMono-Regular`) и `FontFamily::SansBold` (`Ubuntu-Bold`) со стилизованными текстовыми фрагментами и ссылками (`text/runs.rs`, `form_document/text_runs.rs`, `runtime/links.rs`), якорные немодальные попапы/меню/тултипы (`runtime/popup.rs`), приём внешних обновлений `StatePush` в `Runtime` и режим `HostMode::Application` с `NativeWaker` в `native_window.rs`.
- В `crates/gpu-kit` объединены GPU-примитивы (`backend`, `model`, `material_render`, `material_texture`, `exposure`, `histogram`, `hud`, `filmstrip`) поверх общего строителя render/compute пайплайнов и экранных uniform/vertex буферов (`pipeline.rs`) и `memory-budget`.
- В `packages/svet` разделены `svet-wasm` (`graph`, `lis`, `gpu_math`, `ffi`, `native`, `tests`), движок Forma (`forma-ast`, `forma-aot`, `forma-dom`, `forma`) и 3D/графические модули GPU (`gpu`, `gpu-scene3d`, `gpu-sparkline`, `gpu-resident`) без взаимных реэкспортных циклов; ускорена пакетная очистка `For`, снижен baseline `typecheck:indexed` (741 → 621).
- Линтеры и контроль качества:
  - Корневой JavaScript: 0 подавлений в `eslint-suppressions.json` (`{}`), граф модулей проверяется в `test/module-graph.test.js` и `test/check-layering.test.js`.
  - Rust workspace Clippy ratchet (`scripts/clippy-ratchet.mjs`): **0 предупреждений во всём workspace** (`docs/refactor/lint-baseline.json = {}`).
- Серверная часть Studio вынесена из `vite.config.js` в модульный слой `dev-server/` (`wasm-middleware.js`, `mcp-bridge.js`, `runner-ws.js`, `index.js`), а `vite.config.js` содержит только декларативную конфигурацию сборки и dev-сервера.
- В `crates/forma/src/form_document` выделен быстрый контур синхронизации биндингов (`Document::sync_runtime_bindings`, -74% аллокаций на горячем пути) и атомарная транзакционная запись батчей в `binding.rs`.
- Протокольный слой `protocol/` (`enums.js`, `mcp-tools.js`) объединяет 45 MCP-инструментов для `mcp/server.js`, `mcp/smoke.js` и доменных обработчиков `src/mcp-commands-{project,design,runtime}.js`.
- Один релизный тег `vX.Y.Z` на библиотеку и Studio; единый стиль rustfmt (style edition 2024, ширина 100) и Prettier (ширина 100).

## Не сделано

- Дальнейшая декомпозиция оконных хостов (`crates/forma/src/native_window.rs`, `crates/forma/src/main.rs`) и UI-оболочки `src/main.js`.
