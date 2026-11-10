# Целевая архитектура

Полное описание слоёв, деревьев модулей и правил зависимостей находится в `PLAN.md`, раздел «Целевая архитектура».
Здесь фиксируется только то, что уже соответствует цели, и то, что ещё нет.

## Сделано

- Каталог ядра называется так же, как пакет: `crates/forma`.
- Корневой Cargo workspace: один lock, один `target/`, общий профиль `release`; `gpu-compute`, `gpu-kit` и `svet-wasm` вне его (ADR 0001).
- Все крейты собираются и тестируются гейтом (`scripts/full-gate.mjs`) и CI, включая проверку слоёв крейтов (`scripts/check-layering.mjs`) и `cargo deny`.
- Строгая послойная архитектура внутри `crates/forma/src` без исключений (`KNOWN_VIOLATIONS = &[]` в `crates/forma/tests/architecture.rs`); подсистемы `form_document/*`, `control_state/*` и `runtime/*` выделены в чёткие модули.
- В `packages/svet` разделены `svet-wasm` (`graph`, `lis`, `gpu_math`, `ffi`, `tests`), движок Forma (`forma-ast`, `forma-aot`, `forma-dom`, `forma`) и 3D/графические модули GPU (`gpu-scene3d`, `gpu-sparkline`, `gpu`); снижен baseline `typecheck:indexed` (741 → 627).
- Линтеры и контроль качества:
  - Корневой JavaScript: 0 подавлений в `eslint-suppressions.json` (`{}`), граф модулей проверяется в `test/module-graph.test.js`.
  - Rust workspace Clippy ratchet (`scripts/clippy-ratchet.mjs`): **0 предупреждений во всём workspace** (`docs/refactor/lint-baseline.json = {}`).
- Серверная часть Studio вынесена из `vite.config.js` в модульный слой `dev-server/` (`wasm-middleware.js`, `mcp-bridge.js`, `runner-ws.js`, `index.js`), а `vite.config.js` содержит только декларативную конфигурацию сборки и dev-сервера.
- В `crates/forma/src/form_document` выделен быстрый контур синхронизации биндингов (`Document::sync_runtime_bindings`, -74% аллокаций на горячем пути) и атомарная транзакционная запись батчей в `binding.rs`.
- Протокольный слой `protocol/` (`enums.js`, `mcp-tools.js`) объединяет 45 MCP-инструментов для `mcp/server.js`, `mcp/smoke.js` и доменных обработчиков `src/mcp-commands-{project,design,runtime}.js`.
- Один релизный тег `vX.Y.Z` на библиотеку и Studio; единый стиль rustfmt (style edition 2024, ширина 100) и Prettier (ширина 100).

## Не сделано

- Дальнейшая декомпозиция оконных хостов (`crates/forma/src/native_window.rs`, `crates/forma/src/main.rs`) и UI-оболочки `src/main.js`.
