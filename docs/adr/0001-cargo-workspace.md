# 0001. Корневой Cargo workspace

## Контекст

У каждого крейта был свой манифест, часть имела собственные lock-файлы, каталоги сборки были разбросаны
(`crates/forma/target`, `crates/forma/examples/*/target`, `crates/gpu-runtime/target`). Десять крейтов в `crates/`
не собирались ни гейтом, ни CI. Единых профилей и места для линтов не было.

## Решение

Корневой `Cargo.toml` объявляет единый workspace: `crates/forma`, `crates/forma-build`, `crates/forma-mcp`, оба
примера (`binding-app`, `language-app`), все GPU/графические и вычислительные крейты (`gpu-shader`,
`gpu-runtime`, `gpu-compute`, `gpu-kit`, `raster-core`, `math-core`, `math-compute`, `tensor-core`,
`compute-core`, `compute-mlx`, `compute-cuda`), `memory-budget`, `swap-store` и три крейта Svet
(`svet-graph`, `svet-reconcile`, `svet-gpu-arena`).
Один корневой `Cargo.lock`, один `target/` и общий профиль `release` (`lto = "fat"`, `codegen-units = 1`).
Минимальная поддерживаемая версия стабильного компилятора для потребителей Cargo зафиксирована как
`rust-version = "1.92"`, тогда как `rust-toolchain.toml` (`1.97.1`) закрепляет точный toolchain CI для
воспроизводимой сборки WASM и Clippy-ratchet.

Ранее отложенные `crates/gpu-compute` и `crates/gpu-kit` возвращены в корневой workspace после обновления
`wasm-bindgen` до `=0.2.130` (с CLI `0.2.130`) и выравнивания `wgpu` Linux-ветки `forma` на `=30.0.1`.
Legacy WebView-хост `native-app` (`tao`/`wry` + `FORMA_ACTIONS`) выведен из эксплуатации в пользу
`forma::native_window`.

Вне workspace остаются только:

- `packages/svet` (крейт `svet-wasm`): самодостаточный npm-пакет с собственным lock-файлом и release-профилем
  (`opt-level = "z"`, `panic = "abort"`), проверяемый по отдельному контуру CI.
- `.forma` и каталоги с замороженными benchmark/qualification-снимками исходников.

## Последствия

- `scripts/versions.mjs`, `tokenizer.mjs`, `runtime-digest.mjs`, `build-wasm.mjs`, бенч-скрипты, оба мобильных
  скрипта и `ci.yml` читают единый корневой `Cargo.lock` и `target/`.
- Примеры `binding-app` и `language-app` входят в `default-members` и генерируют Rust-формы через
  `crates/forma-build` прямо из `build.rs` без Node.js.
- Отдельные lock-файлы `crates/gpu-compute/Cargo.lock` и `crates/gpu-kit/Cargo.lock` удалены; весь workspace
  проверяется общим `cargo deny --locked check` и `scripts/clippy-ratchet.mjs`.
