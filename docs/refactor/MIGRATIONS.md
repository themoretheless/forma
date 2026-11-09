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
