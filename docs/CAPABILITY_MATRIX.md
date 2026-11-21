# Матрица возможностей и верификации Forma (`0.3.8`)

Статусы возможностей на срезе `2026-11-21` (Wave 7B, все 35 строк [инвентаризации](refactor/INVENTORY.md) в статусе `done`, `npm run gate` `30/30`):

- **`validated`** — реализовано в коде, покрыто автотестами / кроссбраузерными / аппаратно-профильными проверками в этом репозитории и воспроизводится указанной командой.
- **`implemented`** — реализовано в коде и компилируется / проверяется структурно, без отдельного сквозного аппаратного прогона.
- **`partial`** — работает в проверенных границах или на заменяющем бэкенде / нагрузке хоста, но имеет явно зафиксированное аппаратное, платформенное или нагрузочное ограничение.
- **`unsupported`** — сознательно не реализовано / не публикуется на данном этапе (`0.3.8`).

Связанные документы: [BASELINE.md](refactor/BASELINE.md), [INVENTORY.md](refactor/INVENTORY.md), [BUGS.md](refactor/BUGS.md), [MIGRATIONS.md](refactor/MIGRATIONS.md), [ROADMAP.md](ROADMAP.md), [PRODUCTION_READINESS.md](../PRODUCTION_READINESS.md).

---

## 1. Язык `.ui`, `SceneIr`, свойства и генератор `forma-build`

| Возможность | Статус | Что подтверждено и границы | Команда воспроизведения |
|---|---|---|---|
| Лексер и грамматика `.ui` (shared Rust + Lezer WASM) | `validated` | Общий парсер из pinned-ревизии `tokenizer` (`crates/forma` + `@themoretheless/tokenizer-forma`), интерполяция строк `S-7`, `tokenizer check` | `node scripts/tokenizer.mjs check && cargo test --offline --locked -p forma --test eval_builtins` |
| Типизированные `prop`, `event`, `enum`, `match`, `if`/`else`, `for … key …`, `forward`, `required prop`, `#[design]` | `validated` | Golden- и интеграционные тесты языка, компилятора Studio и `library_components` | `node --test test/language*.test.js && cargo test --offline --locked -p forma --test library_components` |
| Канонический реестр свойств (`PROPERTIES`, `InvalidationClass`) | `validated` | Анализ зависимостей invalidation (`layout` / `paint` / `composition`) без пересборки узлов на смене цвета | `cargo test --offline --locked -p forma --lib form_document::registry` |
| Типизированный `SceneIr` и инкрементальная `reconcile_scene_ir` | `validated` | Замена сцен и интернированные `Arc<Template>`, общий снимок `forma-scene-artifact/2`, WASM-контракт | `cargo test --offline --locked -p forma --test retained_pipeline --test scene_artifact` |
| Автономная кодогенерация Rust-форм из `build.rs` (`forma-build`) | `validated` | Без Node.js на этапе сборки: сквозной `language-app` и нативное `binding-app` (8/8 тестов) | `cargo run --locked --offline --manifest-path crates/forma/examples/language-app/Cargo.toml && cargo test --locked --offline --manifest-path crates/forma/examples/binding-app/Cargo.toml --features native` |
| Многострочный Rich Text (`RTF`) и каскадные `style`-блоки (вне `.ui` токенов) | `unsupported` | Зафиксированное ограничение языка (`README.md`, `LANGUAGE.md`) | — |

---

## 2. Runtime `forma`, 5-этапный рендер, типографика, ввод и `control_state`

| Возможность | Статус | Что подтверждено и границы | Команда воспроизведения |
|---|---|---|---|
| Постоянное дерево UI и 5-этапный пайплайн (`expand -> measure -> arrange -> paint -> hit_test`) | `validated` | Потайловый (`32×32`) CPU-растр (`F-15`), динамические `ScrollArea` слоты (`F-16`), общий `DisplayList` IR и `ResourceCache` под `memory-budget` | `cargo test --offline --locked -p forma --features gpu --test retained_pipeline --test display_list` |
| Рендер CPU (эталон) + `wgpu`/Metal с автоматическим `render_rgba_with_cpu_fallback` | `validated` | Согласованность пикселей на 1×/1.25×/2× DPI (`#[ignore]` на Metal и Linux Vulkan `lavapipe`) | `cargo test --offline --locked -p forma --features gpu --lib --tests -- --ignored` |
| Типографика и emoji (`skrifa 0.44.0`, BiDi UAX#9, UAX#14/kinsoku, OpenType `GSUB`/`GPOS`, COLRv0/v1/SBIX/CBDT) | `validated` | RTL-каретка, арабское/тайское сегментирование (`F-12`), переменный вес, fallback без `ttf-parser` | `cargo test --offline --locked -p forma --features native --test typography_scripts --test grapheme_conformance --test word_conformance` |
| Нативный хост окна (`forma::native_window`), ввод, субкластерный IME и AccessKit | `validated` | Десктопные клавиши (Apple `Meta` vs Ctrl/AltGr), обрезка A11y-границ (`F-13`), сохранение скролла (`F-14`) | `cargo test --offline --locked -p forma --features native --lib --bin forma --test native_input_qualification` |
| Модели состояний контролов (`crates/forma/src/control_state/*`) | `validated` | Все 15 подмодулей; исправлены 4 бага переполнения/`clamp`/`NaN` (`F-17`), 0 нетестовых `.unwrap()` | `cargo test --offline --locked -p forma --lib control_state` |
| Адаптация под iOS и Android (`aarch64-apple-ios-sim`, `aarch64-linux-android`, `emulation.rs`) | `partial` | Кросс-компиляция в `full-gate.mjs` + десктопная эмуляция геометрии/хрома (`native_window/emulation.rs`); нет прогона на ферме физических мобильных ОС | `node scripts/full-gate.mjs mobile-android mobile-ios` |
| Вложенный `Scroll` внутри `Scroll` и полная реконверсия системного IME / non-modal popups | `partial` | Зафиксировано в `F-11` (`tests/flat_scene_limits.rs`) и `README.md`; одиночный `Scroll` и модальный слой `Modal` полностью поддержаны | `cargo test --offline --locked -p forma --test flat_scene_limits` |
| Времена 9 бенчмарков на idle-хосте (`vm.loadavg < 1.0`) | `partial` | На `2026-11-21` все 9 бенчмарков сняты последовательно при внешней нагрузке `vm.loadavg` `5.96–17.68` ([BASELINE.md](refactor/BASELINE.md)); команды переснятия приведены там же | См. команду в [BASELINE.md](refactor/BASELINE.md#воспроизведение-9-замеров-на-свободном-хосте-vmloadavg--10) |

---

## 3. GPU-стек и вычислительные крейты

| Возможность | Статус | Что подтверждено и границы | Команда воспроизведения |
|---|---|---|---|
| `gpu-shader`: парсер WGSL, `Validator`, генераторы MSL, SPIR-V 1.3, HLSL SM 5.1/6.0, GLSL 450/ES310, WGSL | `validated` | Zero-Metal-compile для нетипизированного WGSL, golden по 123 шейдерам, 33 unit/integration теста | `cargo test --offline --locked -p gpu-shader` |
| `gpu-runtime`: native Metal (`macOS`/`iOS`) + portable `Vulkan` / `DX12` / `OpenGL/GLES` / `BrowserWebGPU` | `validated` | Signal-driven завершение (2000 отправок, 0 blocking waits), 0 предупреждений на 4 таргетах (`wasm32` вкл.), `portable_backends` на `lavapipe`/DX12/GLES в CI | `cargo test --offline --locked -p gpu-runtime && cargo test --offline --locked -p gpu-runtime --lib -- --ignored` |
| `gpu-compute`, `gpu-kit`, `raster-core` (`#![forbid(unsafe_code)]`, `RasterError`) | `validated` | Metal compute/timestamp-тесты + PBR/HUD + потайловый растр без `.unwrap()` | `COMPUTE_REQUIRE_GPU=1 COMPUTE_REQUIRE_TIMESTAMPS=1 cargo test --offline --locked -p gpu-compute -p raster-core && cargo test --offline --locked -p gpu-kit -- --ignored` |
| Чистая математика и тензоры (`math-core`, `tensor-core`, `memory-budget`, `swap-store`) | `validated` | Конформанс-наборы, `#![forbid(unsafe_code)]` на чистых крейтах, 0 `#[allow(...)]` в `math-core`, `90,9–93,6 %` покрытие | `cargo test --offline --locked -p math-core -p tensor-core --features tensor-core/conformance -p memory-budget -p swap-store` |
| Вычислительные бэкенды (`compute-core`, `compute-mlx`, `math-compute`) | `validated` | `compute-mlx` на Apple Silicon (`COMPUTE_REQUIRE_MLX=1`), единая валидация WGSL без двойного разбора, 100 % `// SAFETY:` | `COMPUTE_REQUIRE_GPU=1 COMPUTE_REQUIRE_MLX=1 cargo test --offline --locked -p compute-core -p compute-mlx -p math-compute --features math-compute/gpu,math-compute/tensor,math-compute/tensor-wgsl` |
| CUDA / RTX бэкенд (`compute-cuda`, `math-compute/cuda`, `gpu_runtime::cuda`) | `partial` | 180+ хост-тестов (`deny(unsafe_op_in_unsafe_fn)`, 43/43 `// SAFETY:`, динамический `probe_driver_available`, отказ `CudaPtx`), **нет физического NVIDIA CUDA GPU** в локальном окружении и CI | `cargo test --offline --locked -p compute-cuda` |

---

## 4. Движок Svet (`packages/svet` и Rust-крейты `svet-*`)

| Возможность | Статус | Что подтверждено и границы | Команда воспроизведения |
|---|---|---|---|
| Rust reactive DAG / согласование / арена (`svet-graph`, `svet-reconcile`, `svet-gpu-arena`, `svet-wasm`) | `validated` | Воспроизводимая сборка `svet_wasm.wasm` (стек 64 KiB, побайтово идентично из разных checkout и `CARGO_TARGET_DIR`, `S-9`) | `npm --prefix packages/svet run build && cargo test --locked --manifest-path packages/svet/crates/svet-wasm/Cargo.toml` |
| Модульное TS-ядро (`reactivity-*`, `dom-*`, `advanced-*`, `forma-expr-*`, `resumable`, `ssr-*`, `gpu-*`) | `validated` | 127 тестовых файлов / 1 560 тестов (82,02 % линий TS), строгий DAG и пороги длины функций, `0` предупреждений ESLint | `npm --prefix packages/svet run lint && npm --prefix packages/svet test` |
| Кроссбраузерный DOM, ввод, IME, router burst (`S-17`) и WebGPU resident (`product-app-bench`, `resident-app-bench`) | `validated` | Квалифицировано в Chromium 159, WebKit 27.2 и Firefox 159 (`packages/svet/browsers`: `BROWSER_MATRIX.md`, `cross-browser-matrix-qualification-2026-11-20.json`) | `node packages/svet/scripts/verify-cross-browser-matrix.mjs` *(требует установленных Playwright-браузеров)* |
| Multi-realm V8 GC, потоковый Edge SSR (`workerd 1.20260730.1`) и HTTP 64K-stack (`S-16`) | `validated` | 720/720 WeakRefs за 1 GC-раунд, 432 транспортных запроса без утечек, HTTP soak 5 008 req с `postWarmupPeakGrowthBytes: 0` | `npm --prefix packages/svet run test:actions && python3 packages/svet/scripts/test_analyze_http_soak.py` |
| Потребительский npm-tarball (13 exports, типы `.d.ts`, `sideEffects`) | `validated` | Установка `.tgz` в изолированном потребителе и проверка всех публичных точек входа | `npm --prefix packages/svet run test:package` |
| Edge SSR smoke-раннер по произвольному каталогу (`npm run test:edge`) | `partial` | `packages/svet/scripts/check-edge-ssr.mjs` требует аргумент `<EDGE_DEPS_DIR>` и не входит в `npm run gate` | `node packages/svet/scripts/check-edge-ssr.mjs <EDGE_DEPS_DIR>` |

---

## 5. Forma Studio, MCP и безопасность раннеров

| Возможность | Статус | Что подтверждено и границы | Команда воспроизведения |
|---|---|---|---|
| Фича-архитектура Studio (DAG `src/{shared,compiler,output,editor,inspector,canvas,preview,workspace,tree,shell,app}` + `src/main.js`) | `validated` | Проверка графа достижимости, отсутствия циклов и неиспользуемых зависимостей (`module-graph.test.js`), 89,62 % покрытия исходников | `npm test && npm run lint` |
| Послойная статическая типизация JSDoc (`// @ts-check` поверх zero-build JS) | `validated` | `119` модулей проверяются `tsc -p jsconfig.json --noEmit` без ломки dev-режима; включено в `gate` и CI | `npm run typecheck` |
| Интерактивный дизайнер, холст, инспектор, дерево контролов и нативная оболочка (`studio-window.js`) | `validated` | Включая пакетную правку, группировку, выравнивание, дизайн-состояния и F9-инспекцию нативного окна | `npm run studio:native -- --svet` *(интерактивное GUI-окно)* |
| Единый Rust MCP-сервер `forma-mcp` + bridge IDE | `validated` | `57` инструментов (`45` Studio IDE в `protocol/mcp-tools.json` + `12` встроенных runtime-инструментов) и `3` ресурса (`forma://language`, `forma://ui/source`, `forma://ui/status`) | `cargo test --offline --locked -p forma-mcp` |
| Изолированный раннер `design-data` (`S-20`) и экспорт автономного Cargo-проекта (`S-19`) | `validated` | Однопроходный лексический сканер `validateDesignRustSource`, `sandbox-exec`/`bwrap` с очищенным `env`, `sessionTrust`; CRLF и цифровые имена | `node --test test/design-data-runner.test.js test/design-request.test.js test/native-export.test.js` |
| Полноценный Rust LSP / DAP внутри редактора Studio | `unsupported` | Зафиксированное ограничение Studio (`README.md`) | — |

---

## 6. Цепочка поставок, упаковка релиза и кроссплатформенный CI

| Возможность | Статус | Что подтверждено и границы | Команда воспроизведения |
|---|---|---|---|
| Политика зависимостей, лицензий и уязвимостей (`cargo-deny`) | `validated` | Заменены `ttf-parser` (`skrifa 0.44.0`) и `quick-xml` (`0.41.0` через `wayland-scanner 0.31.11`); убраны `RUSTSEC-2026-0194`, `-0195`, `-0192` | `cargo deny --offline --locked check` |
| Транзитивные информационные уведомления `zbus 5.19` на Linux AT-SPI (`RUSTSEC-2026-0370..-0372`) | `partial` | Относятся к объединению крейтов в будущей `zbus 6.0` через `accesskit_winit -> accesskit_unix -> atspi`; оставлены в `deny.toml` до апстрим-миграции | `cargo deny --locked check` |
| Проверенный единый релиз-пакет (WASM + Studio + provenance `build-info.json` + SHA-256) | `validated` | Проверяет совпадение `HEAD`, `dirty: false`, pinned `tokenizer` rev и равенство `public/vector-pkg` ≡ `dist/vector-pkg` | `npm run build:wasm && npm run build && CI=true npm run package:release` |
| Кроссплатформенная матрица GitHub Actions (`CI`) | `validated` | 8 job'ов: `Crates` и `Forma` на `ubuntu-24.04` (с Vulkan `lavapipe` / GLES), `macos-15` (с `clippy-ratchet`) и `windows-2022` (с DX12), `Studio + actual WASM`, `Svet` | `.github/workflows/ci.yml` + `unset FORMA_TOKENIZER_DIR && npm run gate` (`30/30`) |
| Публикация в `crates.io` и `npm` | `unsupported` | По проекту крейты и релиз распространяются через закреплённые git-теги / ветку `release` и `.release/*.tar.gz` (`RELEASING.md`) | — |
