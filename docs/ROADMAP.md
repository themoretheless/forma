# Единый план развития Forma

Единственный файл планов по всему репозиторию (`Forma`, `Studio`, `Svet`, `GPU` и вычислительные
крейты) на 2026-11-20 (`4ddf618a`). Здесь собрана только незавершённая работа и действующие
правила выполнения; история выполненных этапов, базовые замеры, дефекты и изменения контрактов
хранятся в [ARCHITECTURE.md](refactor/ARCHITECTURE.md), [BASELINE.md](refactor/BASELINE.md),
[BUGS.md](refactor/BUGS.md), [INVENTORY.md](refactor/INVENTORY.md),
[MIGRATIONS.md](refactor/MIGRATIONS.md), [SNAPSHOTS.md](refactor/SNAPSHOTS.md),
[PRODUCTION_READINESS.md](../PRODUCTION_READINESS.md) и
[packages/svet/DEVELOPMENT.md](../packages/svet/DEVELOPMENT.md).

## 1. Язык `.ui` и компилятор

- [ ] Один источник истины для `.ui`: сделать `themoretheless-tokenizer-forma` и Rust
  `form_document` единственным источником семантики и проверки типов, переведя Studio
  (`src/components.js`, `src/compiler/*`) и Svet (`forma-ast.ts`, `forma-expr.ts`) на общий
  AST/IR; расширить `test/language-conformance.test.js` сверх 26 случаев раскладки на выражения,
  компоненты и Svet и закрыть 6 записанных расхождений в `test/golden/language-conformance.txt`
  (трёхзначный `padding` и двузначный `gap` корневого `Frame`, `width: 0`, ширина корня > 4096,
  `clip: true`, а также отображение ошибок рантайма на позиции исходного `.ui`).
- [ ] Типизированный `SceneIr` от `Document` к `Runtime` вместо обратной сериализации
  `serialize_into` и текстового транспорта `FORMA-TEMPLATES-1` (218 мкс из 465 мкс на 256
  контролах) — через строитель валидированной сцены в `themoretheless-tokenizer-forma` либо общую
  валидацию в Rust; `from_sources` остаётся внешним текстовым фронтендом.
- [ ] Стабильные идентификаторы узлов между перекомпиляциями: сопоставление snapshot и origin для
  выделения, фокуса, инспектора и MCP без сброса состояния взаимодействия при правке.
- [ ] Внутренняя чистка `form_document` и `binding` (см. [INVENTORY.md](refactor/INVENTORY.md)):
  переход на `NodeKind` вместо строковых имён узлов, типизированные ошибки вместо проверки
  префиксов строк, разбиение `eval_mode` и `Form::sync` / `Form::update` на подмодули.
- [ ] Реестр свойств и bindings (типы, стили, зависимости, точечная инвалидация).

## 2. Runtime Forma, ввод и рендер

- [ ] Квалификация нативного и системного ввода (сценарии в [MIGRATIONS.md](refactor/MIGRATIONS.md),
  раздел 19): настоящие события указателя (помимо AccessKit), системный CJK marked-text IME
  (`Preedit`/`Commit`/отмена), системный буфер обмена и озвучивание screen reader, переходы между
  физическими DPI, несколько окон, iOS simulator/устройство и физический touch; отделить цикл
  оконного хоста `native_window.rs` от платформенной эмуляции.
- [ ] Полнота текста и типографики: абзац справа налево, переходы каретки на границах разнонаправленных
  прогонов, расширенный арабский алфавит, полный shaping и кернинг сложных письменностей, отрисовка
  цветных emoji, правила переноса строк и расширенные визуальные эталоны CJK/fallback.
- [ ] Стоимость больших форм и длительных сессий: повторные замеры кадра и пересборки на
  8/32/96/256 контролах на спокойном хосте (`load < 1`), снижение стоимости замены `Runtime` при
  редактировании и квалификация плато RSS, GPU-памяти и задержки ввод → экран в долгом OS-окне.
- [ ] Пайплайн удерживаемого рендера (строго по порядку):
  1. Постоянное дерево UI с раздельными флагами изменения `layout`, `paint` и `composition` (смена
     цвета не должна перестраивать раскладку и узлы).
  2. Независимый от backend список рисования с `bounds`, `transform`, `clip-id` и `resource-id`,
     единый для CPU, GPU и диагностики Studio.
  3. Кэши по элементам/тайлам, dirty rectangles и ограничение слоёв вложенного `clip` границами
     узла вместо выделения на весь viewport и полной инвалидации при прокрутке.
  4. Общий glyph atlas и кэш SVG под глобальным байтовым бюджетом (`memory-budget`) с
     LRU-вытеснением.
  5. Расширение GPU-бэкенда через `wgpu` при сохранении CPU-пути как эталона и fallback.

## 3. GPU-стек (`gpu-shader`, `gpu-runtime`, `gpu-compute`)

- [ ] `gpu-shader`: настоящая проверка типов в `gpu_shader::valid::Validator`, чтобы ill-typed
  WGSL не доходил до Metal; завершить декомпозицию крупных функций в `msl.rs` и `wgsl.rs`.
- [ ] `gpu-runtime` и `gpu-compute`: расставить `// SAFETY:` помодульно со снятием crate-wide
  `allow(unsafe_op_in_unsafe_fn)`, отделить не-Apple Metal-заглушки для снятия условного
  `allow(dead_code, unused_variables)`, разбить длинные функции кодирования команд/render pass,
  добавить `Error::source()` в `gpu-compute` и квалифицировать физические сбои GPU, потерю
  устройства / смену адаптера, idle-уведомления удаления устройства и latency/RSS
  signal-driven завершения.
- [ ] Дополнительные GPU-бэкенды (по проверяемости в CI, каждый с ADR и тестовой матрицей):
  1. SPIR-V + Vulkan (Linux CI на lavapipe).
  2. HLSL + DX12 (Windows CI раннер).
  3. GLSL + OpenGL, затем WebGPU (`wasm`).
  4. CUDA и RTX (только при наличии NVIDIA-хоста).

## 4. Svet (`packages/svet`)

- [ ] Браузерный ввод и кроссбраузерная матрица (`packages/svet/BROWSER_MATRIX.md`): проверить
  доверенный клавиатурный ввод по словам, CJK IME-композицию, AltGr и не-Apple Ctrl-раскладки в
  `src/vector-preview.js` на Chromium, WebKit и Firefox.
- [ ] Интеграции рантайма и SSR: soak-тест GC across-copy и нескольких realms, захват асинхронного
  контекста после `await`, потоковые SSR/Edge-носители под медленными клиентами и отменой,
  burst-нагрузка роутера и разбор причины исторического отказа HTTP external memory.
- [ ] Декомпозиция движка и внутренние слои: разбить сборку экземпляра с HMR в `instantiateForma`
  (`forma.ts`, ~550 строк), `createCanvas3D` (`gpu-scene3d.ts`, ~880 строк) и остаток
  асинхронной логики в `gpu.ts`; закрепить слои `reactivity` → `dom` → `gpu` → `forma`.
- [ ] Эффективность компилятора и реактивного состояния: не оборачивать чистые статические вызовы
  в реактивные thunk в `compiler.ts` (`hasEagerCall`) и убрать дублирование графовых рёбер между JS
  (`reactivity.ts`) и линейной памятью `svet-wasm`.
- [ ] Воспроизводимое происхождение `svet-wasm` (`S-9`): убрать зависимость байтов `svet-wasm` от
  каталога checkout (включить `svet-wasm` в корневой workspace либо проверять экспорты и дайджест
  исходников вместо чувствительных к пути байтов).
- [ ] Продуктовые бенчмарки реальных приложений: startup, p95 задержка взаимодействия, frame time и
  память.

## 5. Studio и dev-сервер

- [ ] Декомпозировать `src/main.js` (~1932 строки) до чистой композиции над `app/state.js`,
  сервисами с инъекцией портов и реестром команд (удалив скрытый дублирующий шаблон старого
  заголовка); разложить `src/` по фичам (`workspace`, `editor`, `preview`, `canvas`, `inspector`,
  `tree`, `output`, `shell`) с проверкой в `test/module-graph.test.js`.
- [ ] Послойно включить `// @ts-check` + JSDoc для JS в Studio (добавить проверку типов без ломки
  zero-build dev-режима).
- [ ] Характеризация и безопасность раннеров: покрыть граничные случаи `buildNativeRustExport`,
  закрепить оставшиеся тексты `throw` компилятора и заменить обходимый regex-blacklist в
  `design-data-runner.js` на явное доверие проекту на уровне сессии / ОС-песочницу (`sandbox-exec` /
  `bwrap`) или AST-валидацию.

## 6. Вычислительные крейты и финальная чистка

- [ ] Документировать FFI `// SAFETY:` в `compute-mlx` и соседних крейтах; заменить оставшиеся
  достижимые библиотечные `expect` / `unwrap` типизированными ошибками.
- [ ] Внутренний долг крейтов ([INVENTORY.md](refactor/INVENTORY.md)): табличный
  `RasterPipelines::new` и вывод тестов во временные каталоги в `raster-core`, устранение дублей
  `gpu`/`cuda` в `math-compute`, разбиение `spatial.rs` и сокращение `allow` в `math-core`,
  разбиение `impl Tensor` в `tensor-core` и устранение двойного разбора WGSL в `compute-core`.
- [ ] Цепочка поставок и закрытие базовой линии: отслеживать/заменить неподдерживаемые
  транзитивные зависимости (`quick-xml`, `ttf-parser`, `zvariant`), снять покрытие Rust/Svet и
  замеры на спокойном хосте в [BASELINE.md](refactor/BASELINE.md), а также закрыть оставшиеся
  строки `todo` и `в работе` в [INVENTORY.md](refactor/INVENTORY.md).

## 7. Выпуск и очередь выполнения

- [ ] Release candidate на одной чистой ревизии: чистая автономная установка, цепочка Rust → WASM
  → пакеты Studio и `svet-engine`, provenance, exports и types, browser/native/HTTP-гейты и зелёный
  удалённый CI на Linux, macOS и Windows.
- [ ] Опубликовать матрицу `implemented / validated / partial / unsupported` с точными командами
  воспроизведения.

### Очередь выполнения

1. Единая семантика `.ui` и типизированный `SceneIr` (раздел 1).
2. Нативный ввод/типографика и переход на удерживаемый рендер (раздел 2).
3. Release candidate и публикация матрицы возможностей (раздел 7); декомпозиция Studio/Svet, новые
   GPU-бэкенды и чистка вычислительных крейтов идут по своим трекам параллельно либо после
   ключевых развязок.

### Действующие инварианты

- Контракты генераторов, `Runtime`, WASM-экспортов, JNI-символов и имён MCP-инструментов меняются
  только с записью в [MIGRATIONS.md](refactor/MIGRATIONS.md).
- Найденный баг фиксируется в [BUGS.md](refactor/BUGS.md) и закрывается вместе с падающим тестом.
- Счётчики предупреждений (`clippy-ratchet`, `eslint`) могут только убывать; `allow` не
  добавляются.
- После изменений в `crates/forma/src`, `Cargo.toml` или `Cargo.lock` пересобирать WASM
  (`npm run build:wasm`) перед финальным гейтом.
- В коммитах по `compute-cuda` без проверки на устройстве явно указывать, что на CUDA-железе
  запуск не выполнялся; сообщения коммитов — обычное предложение без префиксов типа.
