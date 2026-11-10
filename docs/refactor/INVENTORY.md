# Инвентаризация

Статусы: `todo`, `в работе`, `done`, `отложено`. Запахи взяты из разведки чтением кода; построчный проход каждого
метода по чеклисту ещё не выполнен ни для одного модуля, поэтому все строки в статусе `todo`.
Номера строк в описаниях разведки относятся к коду до форматирования; ориентироваться по именам.

| Модуль | Строк | Тестов | Главные запахи | Приоритет | Статус |
|---|---:|---|---|---|---|
| crates/forma/src/form_document.rs + control_flow/visual/scene/render/bindings/tests | 548 + 206 + 566 + 610 + 462 + 373 + 2585 | 60 | разбит по уровням рендера, общие селекторы в `control_flow.rs` (F-3 закрыт), облегчённый проход `sync_runtime_bindings` без визуальной инстанциации/сериализации сцены (-74% аллокаций и 3.2x ускорение на 96 контролах); осталось: `NodeKind` вместо строк | 1 | в работе |
| crates/forma/src/runtime.rs + {a11y,modal,raster,wasm_api,tests,raster_tests} | 1320 + 200 + 82 + 109 + 1102 + 1369 + 302 | 41 | a11y, модальные слои, растровый кэш, wasm-фасад и тесты разнесены по подмодулям `runtime/*` | 1 | в работе |
| crates/forma/src/main.rs | 2015 | 4 | второй оконный хост, `window_event` на сотни строк | 1 | todo |
| crates/forma/src/native_window.rs | 3164 | 19 | хост + генерация UI + эмуляция платформ + ввод | 1 | todo |
| crates/forma/src/binding.rs | 1707 | 22 | `unsafe` убран (F-1), пакетная запись атомарна через `prepare_write` (F-8), 0 предупреждений Clippy; осталось: разбиение `Form::sync`/`update` на подмодули | 1 | в работе |
| crates/forma/src/form_document/eval.rs + eval/builtins/* | 505 + 880 | golden на все builtins, 2 теста цвета | диспетчер builtins разбит на 9 семейств; осталось: `eval_mode` (~280 строк), ошибки строками, тесты путей и операторов | 1 | в работе |
| crates/forma/src/form_document/{bindings,compiler,layout,contracts,svg}.rs | — | 0 | нет прямых тестов | 1 | todo |
| crates/forma/src/text.rs | 2298 | 18 | юникод, шейпинг, шрифты, раскладка и растеризатор вместе; цикл с `shape` | 2 | todo |
| crates/forma/src/control_state/* | 2291 в 15 файлах | 13 | разбит по моделям | 2 | done (перемещение); логика не проходилась |
| crates/forma/src/template.rs | — | 14 | лексер, парсер, кэш; публичный элемент после тестового модуля | 2 | todo |
| crates/forma/src/{raster_cache,display_list,lib}.rs | — | 9 / 0 / 17 | цикл с `lib` устранён через `ControlView<'a>` (`KNOWN_VIOLATIONS = &[]` в `tests/architecture.rs`) | 2 | done |
| crates/forma/src/{gpu,mcp,reveal,text_input,native_*}.rs | — | — | не проходились | 3 | todo |
| packages/svet/src/engine/forma{,-ast,-aot,-dom}.ts | 1130 + 686 + 633 + 1126 | через barrel (1527 тестов) | разбит на AST/парсер (`forma-ast`), AOT/валидацию (`forma-aot`), табличный DOM/SVG/биндинги (`forma-dom`) и оркестратор (`forma`); 0 находок `typecheck:indexed` | 1 | done |
| packages/svet/src/engine/gpu{,-scene3d,-sparkline}.ts | 1444 + 1359 + 465 | через barrel | 3D-сцена/камера/рейкастинг (`gpu-scene3d`) и `GpuSparkline` (`gpu-sparkline`) вынесены из ядра GPU-контекста; находки `typecheck:indexed` снижены с 75 до 15 | 2 | done |
| packages/svet/src/engine/reactivity.ts | 2671 | через barrel | горячий путь; повторяющиеся блоки в `signal`/`computed` | 2 | todo |
| packages/svet/src/engine/dom.ts | 2574 | через barrel | `For`, `bindModel`, `bindProp` по 150–260 строк | 2 | todo |
| packages/svet/src/engine/{advanced,forma-expr,forma-reactive,…}.ts | — | — | смешанные ответственности в `advanced`; остальные не проходились | 3 | todo |
| packages/svet/crates/svet-wasm/src/{lib,graph,lis,gpu_math,ffi,tests}.rs | 36 + 1106 + 109 + 638 + 414 + 1236 | 67 | разбит на реактивный DAG (`graph`), LIS-реконсилер (`lis`), арену и SIMD/матрицы (`gpu_math`), WASM FFI (`ffi`) и тесты (`tests`) | 2 | done |
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
