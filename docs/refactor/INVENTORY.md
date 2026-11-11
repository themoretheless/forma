# Инвентаризация

Статусы: `todo`, `в работе`, `done`, `отложено`. Запахи взяты из разведки чтением кода; построчный проход каждого
метода по чеклисту ещё не выполнен ни для одного модуля, поэтому все строки в статусе `todo`.
Номера строк в описаниях разведки относятся к коду до форматирования; ориентироваться по именам.

| Модуль | Строк | Тестов | Главные запахи | Приоритет | Статус |
|---|---:|---|---|---|---|
| crates/forma/src/form_document.rs + control_flow/visual/scene/render/bindings/tests | 548 + 206 + 566 + 610 + 462 + 373 + 2585 | 60 | разбит по уровням рендера, общие селекторы в `control_flow.rs` (F-3 закрыт), облегчённый проход `sync_runtime_bindings` без визуальной инстанциации/сериализации сцены (-74% аллокаций и 3.2x ускорение на 96 контролах); осталось: `NodeKind` вместо строк | 1 | в работе |
| crates/forma/src/runtime.rs + {a11y,canvas,modal,raster,wasm_api,tests,raster_tests} | 1185 + 200 + 247 + 82 + 115 + 1127 + 1369 + 302 | 48 | a11y, canvas-слоты, модальные слои, растровый кэш, wasm-фасад (`StatePush`) и тесты разнесены по подмодулям `runtime/*` | 1 | в работе |
| crates/forma/src/canvas.rs + tests/canvas.rs | 692 + 274 | 10 | прикладная геометрия `CanvasContent` / `CanvasView`, аналитическое покрытие через общую `geometry::column_area` | 2 | done |
| crates/forma/src/main.rs | 2015 | 4 | второй оконный хост, `window_event` на сотни строк | 1 | todo |
| crates/forma/src/native_window.rs + tests.rs | 3675 + 538 | 31 | добавлены `HostMode::Application`, `NativeWaker` и хуки `NativeApp`; осталось разделение оконного хоста и платформенной эмуляции | 1 | в работе |
| crates/forma/src/binding.rs | 1707 | 22 | `unsafe` убран (F-1), пакетная запись атомарна через `prepare_write` (F-8), 0 предупреждений Clippy; осталось: разбиение `Form::sync`/`update` на подмодули | 1 | в работе |
| crates/forma/src/form_document/eval.rs + eval/builtins/* | 505 + 880 | golden на все builtins, 2 теста цвета | диспетчер builtins разбит на 9 семейств; осталось: `eval_mode` (~280 строк), ошибки строками, тесты путей и операторов | 1 | в работе |
| crates/forma/src/form_document/{bindings,compiler,layout,contracts,svg}.rs | — | 7 | добавлены прямые unit-тесты `layout`, `contracts`, `control_flow`, `svg`, `svg_path` и `properties` | 2 | в работе |
| crates/forma/src/{geometry,shape,text}.rs | 110 + 185 + 2410 | 24 | устранён цикл `shape <-> text`, добавлен `FontFamily::Mono`, аналитическое покрытие и hit-test объединены в `geometry.rs` (`column_area`, `round_inside`) | 2 | в работе |
| crates/forma/src/control_state/* | 2291 в 15 файлах | 13 | разбит по моделям | 2 | done (перемещение); логика не проходилась |
| crates/forma/src/template.rs + text_input.rs | 1495 + 630 | 28 | устранён цикл `template <-> text_input` (перенос `Input` в `template.rs`), тестовый модуль перенесён в конец файла | 2 | в работе |
| crates/forma/src/{raster_cache,display_list,lib}.rs | — | 9 / 0 / 17 | строгий послойный DAG (`KNOWN_VIOLATIONS = &[]` в `tests/architecture.rs`) | 2 | done |
| crates/forma/src/{gpu,mcp,reveal,native_*}.rs | — | — | `native_document_graph.rs` избавлен от лишних клонирований и повторного `track()` без смены формы; остальные не проходились | 3 | в работе |
| crates/gpu-kit + crates/swap-store | 5400 + 820 | 32 + 9 | `exposure`, `histogram`, `hud`, `filmstrip`, `model`, `material_*` объединены через общие строители `pipeline.rs` и `extent_fits` | 2 | done |
| packages/svet/src/engine/forma{,-ast,-aot,-dom}.ts | 1130 + 686 + 633 + 1126 | через barrel (1530 тестов) | разбит на AST/парсер (`forma-ast`), AOT/валидацию (`forma-aot`), табличный DOM/SVG/биндинги (`forma-dom`) и оркестратор (`forma`); 0 находок `typecheck:indexed` | 1 | done |
| packages/svet/src/engine/gpu{,-scene3d,-sparkline,-resident}.ts | 1423 + 1870 + 73 + 640 | через barrel | устранён реэкспортный цикл `gpu <-> gpu-scene3d/gpu-sparkline`, находки `typecheck:indexed` снижены до 621 | 2 | done |
| packages/svet/src/engine/reactivity.ts | 2671 | через barrel | горячий путь; повторяющиеся блоки в `signal`/`computed` | 2 | todo |
| packages/svet/src/engine/dom.ts | 2585 | через barrel | добавлена пакетная очистка `For` (`clearRange`); `For`, `bindModel`, `bindProp` по 150–260 строк | 2 | в работе |
| packages/svet/src/engine/{advanced,forma-expr,forma-reactive,…}.ts | — | — | смешанные ответственности в `advanced`; остальные не проходились | 3 | todo |
| crates/svet-{graph,reconcile,gpu-arena} + packages/svet/crates/svet-wasm | 1310 + 110 + 790 + 550 | 47 + 2 + 17 + 6 | Rust-ядро Svet разнесено по отдельным крейтам workspace без GPU/LIS-буферов в `ReactiveGraph`; убран фиктивный `ReactiveGraph::new()` в `pick_snapshot_3d`, добавлена защита от реентерабельного `&mut` и `Box<[u32]>` в FFI | 1 | done |
| src/main.js | 2106 | текстовые | импорты сгруппированы сверху, удалены мёртвые ветки папок дерева (`toggleFolder`, `data-folder`, `expandedDirs`), 0 подавлений ESLint; дальнейшее дробление UI-панелей впереди | 1 | в работе |
| src/components.js + component-template.js | 1726 + 56 | да | сериализация шаблонов и экранирование вынесены в `component-template.js` (S-7 закрыт); осталось дробление `compile()` | 1 | в работе |
| vite.config.js + dev-server/{index,wasm-middleware,mcp-bridge,runner-ws}.js | 49 + 14 + 97 + 90 + 42 | 3 (vite-config) | серверная часть, раздача WASM, MCP HTTP/WS-мост и оркестрация раннеров вынесены в `dev-server/*`, `vite.config.js` оставлен декларативным | 2 | done |
| protocol/mcp-tools.js + mcp/server.js + src/mcp-commands{,-project,-design,-runtime}.js | 350 + 109 + 55 + 58 + 185 + 132 | модульные + контрактные | единый реестр 45 MCP-инструментов без внешних зависимостей и доменные обработчики команд | 2 | done |
| остальные 65 файлов src/ | — | частично | не проходились | 3 | todo |
| crates/gpu-shader | 4460 | golden по 123 шейдерам + unit | zero-alloc разбиение верхнеуровневых элементов WGSL, точные `(line, column)` в `ParseError` (G-9 закрыт), 0 предупреждений Clippy; осталось: разбиение `msl.rs`/`wgsl.rs`, G-7/G-8/G-10 | 1 | в работе |
| crates/gpu-runtime | 9450 | 22 | полные ключи кэша compute/render пайплайнов (G-3), защита от `nil`/OOM буферов (G-4) и переполнения смещений записи (G-5), `# Safety` контракты на FFI, 0 предупреждений Clippy; осталось: G-1, G-2, G-6 | 1 | в работе |
| crates/gpu-compute | 3072 | 25 | мелкие | 3 | todo |
| crates/raster-core | 3936 | 31 | `RasterPipelines::new` 426 строк; тесты пишут в `output/` | 3 | todo |
| crates/math-core | 5916 | 92 | `spatial.rs` на 12 семейств типов, 21 `allow` | 3 | todo |
| crates/math-compute | 5963 | 41 | параллельные реализации gpu/cuda (лидер по дублям) | 3 | todo |
| crates/tensor-core | 9850 | 53 | 0 предупреждений Clippy; остался крупный `impl Tensor` | 3 | в работе |
| crates/compute-core | 11 340 | 166 | кэш ядер `KernelCache` переведён на zero-alloc поиск по заимствованным срезам + `Cow<str>` для WG=256, 0 предупреждений Clippy | 3 | в работе |
| crates/compute-mlx | 6859 | 148 | 84 `unsafe` без SAFETY; `backend()` скопирован в 22 тестах | 3 | todo |
| crates/compute-cuda | 16 309 | 180 (хост) | не проверяется на устройстве | 3 | отложено до появления CUDA-хоста для логики |
