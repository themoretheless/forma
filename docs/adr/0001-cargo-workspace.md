# 0001. Корневой Cargo workspace

## Контекст

У каждого крейта был свой манифест, часть имела собственные lock-файлы, каталоги сборки были разбросаны
(`crates/forma/target`, `crates/forma/examples/*/target`, `crates/gpu-runtime/target`). Десять крейтов в `crates/`
не собирались ни гейтом, ни CI. Единых профилей и места для линтов не было.

## Решение

Корневой `Cargo.toml` объявляет workspace: `crates/forma`, два примера, девять крейтов из `crates/`, `openscad-ui`.
Один `Cargo.lock` (посеян из прежнего lock-файла `forma`, версии её зависимостей не изменились), один `target/`.
Профиль `release` (`lto = "fat"`, `codegen-units = 1`) перенесён из `crates/forma/Cargo.toml` в корень.

Вне workspace остаются:

- `crates/gpu-compute`. Его необязательная фича `native-wgpu` тянет `wgpu 30` с crates.io, которому нужен
  `wasm-bindgen >= 0.2.127`, а `forma` закрепляет `wasm-bindgen = 0.2.125` под установленный CLI. Для членов
  workspace Cargo разрешает все необязательные зависимости, поэтому членство потребовало бы обновить закреплённую
  версию. Как обычная path-зависимость крейт разрешается только с запрошенными фичами, и конфликта нет.
  У него свой `Cargo.lock`, гейт собирает и тестирует его отдельным шагом.
- `crates/gpu-kit`. Вне macOS он строится на `wgpu 30` с crates.io (на macOS на `gpu-runtime`), упирается в
  то же ограничение `wasm-bindgen` и так же держит свой `Cargo.lock` и отдельный шаг гейта.
- `packages/svet` (крейт `svet-wasm`). Это самодостаточный npm-пакет: байты его wasm сверяются с собственным
  lock-файлом и профилем (`opt-level = "z"`, `panic = "abort"`), а его задача CI не делает checkout токенизатора.
- `native-app`: требует `FORMA_ACTIONS` при компиляции и GTK/WebKit на Linux.
- `.forma` и каталоги с замороженными снимками исходников.

## Последствия

- `scripts/versions.mjs`, `tokenizer.mjs`, `runtime-digest.mjs`, `build-wasm.mjs`, бенч-скрипты, оба мобильных
  скрипта и `ci.yml` читают корневой `Cargo.lock` и `target/`.
- Lock-файлы примеров удалены: примеры больше не моделируют «экспортированное приложение со своим lock-файлом».
- В гейте появились шаги `crates-test`, `crates-math-compute`, `crates-mlx`, `crates-gpu-compute`, `openscad-sample`.
- Первый запуск после перехода пересобирает всё в новом `target/`; MCP-сервер из `.mcp.json` при первом старте
  может не уложиться в таймаут подключения, достаточно один раз собрать `forma` заранее.

## Что пересмотреть

Обновление `wasm-bindgen` до 0.2.127+ (вместе с CLI) позволит включить `gpu-compute` в workspace. Это решение
владельца: оно меняет закреплённую версию инструмента сборки.
