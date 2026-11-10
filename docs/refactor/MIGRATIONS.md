# Изменения контрактов

Каждая запись: что изменилось, кого затрагивает, как мигрировать, коммит.

| № | Изменение | Затронуто | Миграция | Коммит |
|---|---|---|---|---|
| 1 | `vector-ui` → `crates/forma` | пути, URL галереи, путь по умолчанию в экспорте | см. раздел 1 | Rename vector-ui to crates/forma |
| 2 | Корневой Cargo workspace | lock-файлы, каталог сборки, профиль release | см. раздел 2 | Put the Rust crates under one Cargo workspace |

Запланировано (см. `PLAN.md`): `vector-ui` → `crates/forma`; общий `Cargo.lock` и `target/`; удаление wasm-класса `Button`;
`Node.kind: NodeKind`; поведение dev-бинарника после слияния хостов; сужение публичного Rust API;
компилятор только из `svet-engine/compiler`; минус 10 wasm-экспортов `svet`; пути модулей Studio и runners;
фасад `createIdeCommands`; пакеты `math-core`/`math-compute`; профиль `svet-wasm`.

## 1. Каталог `vector-ui` переименован в `crates/forma`

- Затронуто: все пути `vector-ui/...` в скриптах, CI, `package.json`, `.mcp.json`, тестах и документации; URL галереи контролов теперь `crates/forma/examples/controls.html`; путь к крейту по умолчанию в native-экспорте (`src/native-export.js`, `src/sample-project.js`) теперь `../crates/forma`.
- Миграция: во внешних проектах заменить `path = ".../vector-ui"` на `path = ".../crates/forma"`; каталог сборки теперь `crates/forma/target`.
- Имя пакета, библиотеки и бинарника (`forma`) не изменилось.

## 2. Корневой Cargo workspace

- Затронуто: `Cargo.lock` теперь один, в корне; каталог сборки `target/` в корне вместо `crates/forma/target`;
  lock-файлы примеров и `gpu-runtime` удалены; профиль `release` объявлен в корневом `Cargo.toml`.
- Миграция: скрипты и CI, читавшие `crates/forma/Cargo.lock` или `crates/forma/target/...`, должны читать
  `Cargo.lock` и `target/...`. Команды `cargo ... --manifest-path crates/forma/Cargo.toml` продолжают работать.
- Подробности и исключения: `docs/adr/0001-cargo-workspace.md`.

## 3. Один релизный тег `vX.Y.Z`

- Затронуто: теги `forma-vX.Y.Z` и `studio-vX.Y.Z` больше не запускают публикацию и не двигают ветку `release`;
  `scripts/versions.mjs check <tag>` принимает только `vX.Y.Z` и требует, чтобы версия совпадала в обоих manifest.
- Миграция: поднимать версию командой `npm run version:set -- all X.Y.Z`, выпускать тегом `vX.Y.Z`.
  GitHub Release один, с двумя архивами. Старые теги остаются в истории.

## 4. OpenSCAD убран из Forma и Studio

- `openscad-ui/` удалён: это была побайтная копия `open-scad-viewer/forma-studio`, оригинал живёт там.
- Studio больше не знает про open-scad-viewer: проекта OpenSCAD нет в списке, по умолчанию (без `?project=`)
  открывается `knowledge-workspace`, у `?project=studio-controls` прежнее поведение.
- Файловый API dev-сервера `/__forma_workspace` (чтение и запись соседней папки `../open-scad-viewer`) удалён
  вместе с деревом папок, которое его использовало. Работа с локальной папкой через «Папка»
  (File System Access API) осталась.
- `npm run studio:native` без аргумента открывает `studio-controls` вместо OpenSCAD.
- Затронуты локальные данные: проект OpenSCAD, сохранённый в браузере под ключами `…:openscad-studio-v5`,
  больше не открывается; данные в хранилище браузера остаются нетронутыми.

## 5. svet в общей релизной версии

- `packages/svet/package.json` (`svet-engine`) и крейт `svet-wasm` больше не держат постоянные номера (1.0.0 и
  0.1.0): они идут в общей версии релиза, `npm run version:set -- all X.Y.Z` поднимает и их, а тег `vX.Y.Z`
  требует совпадения. Выравнивание прошло на 0.3.4, то есть номер `svet-engine` уменьшился с 1.0.0; пакет
  в npm не публиковался, поэтому потребителей сравнения версий нет.

## 6. `gpu-kit`: состояние растеризации одним аргументом

- `MaterialRenderer::prepare` принимает `MaterialRaster { alpha, double_sided, clockwise }` вместо трёх
  отдельных аргументов `alpha`, `double_sided`, `clockwise`. Порядок остальных аргументов прежний.
- Миграция: `prepare(budget, vertices, p, textures, alpha, double_sided, clockwise, cancelled)` →
  `prepare(budget, vertices, p, textures, MaterialRaster { alpha, double_sided, clockwise }, cancelled)`.
  Внутри репозитория вызовов вне тестов `gpu-kit` нет; внешние потребители (код из rrrah) правятся так же.

## 7. Полужирное начертание и отрезки текста

- `text::FontFamily` получил вариант `SansBold`, `template::Template` — поле `links`
  (`Vec<TextLink>`), `text::font_face(4)` теперь возвращает Ubuntu Bold, а не `None`.
- Затронуто: внешний код с исчерпывающим `match` по `FontFamily` и с литералами `Template { .. }`.
- Миграция: добавить ветку `FontFamily::SansBold` (полужирный Ubuntu) и `links: Vec::new()`.
  Встроенный `Text` сцены теперь принимает `fontFamily`, `fontWeight` и `runs`; прежние сцены
  компилируются в те же байты.
