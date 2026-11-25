# Единый план развития Forma

Единственный файл планов по всему репозиторию (`Forma`, `Studio`, `Svet`, `GPU` и вычислительные
крейты) на 2026-11-22 (`d0d1c476`). Здесь собрана только незавершённая работа и действующие
правила выполнения; история выполненных этапов, базовые замеры, дефекты и изменения контрактов
хранятся в [ARCHITECTURE.md](refactor/ARCHITECTURE.md), [BASELINE.md](refactor/BASELINE.md),
[BUGS.md](refactor/BUGS.md), [INVENTORY.md](refactor/INVENTORY.md),
[MIGRATIONS.md](refactor/MIGRATIONS.md), [SNAPSHOTS.md](refactor/SNAPSHOTS.md),
[CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md),
[PRODUCTION_READINESS.md](../PRODUCTION_READINESS.md) и
[packages/svet/DEVELOPMENT.md](../packages/svet/DEVELOPMENT.md).

## Открытая работа

- [ ] Переснять абсолютные времена всех 9 последовательных бенчмарков в idle-сессии хоста
  (`vm.loadavg < 1.0`) по точной команде в [BASELINE.md](refactor/BASELINE.md) (предыдущий срез на
  `2026-11-21` — с `~21×` ускорением замены сцены `55,53 мкс / 789,4 КБ` против
  `1 166,18 мкс / 16,65 МБ` через `reconcile_scene_ir` и `window_bench`: `2 398` present,
  `p50=8,31 мс`, `RSS=99,83→99,89 МБ`, `geometry_uploads=0` — снят при внешней нагрузке хоста
  `vm.loadavg` `5.96–17.68`).

## Действующие инварианты

- Контракты генераторов, `Runtime`, WASM-экспортов, JNI-символов и имён MCP-инструментов меняются
  только с записью в [MIGRATIONS.md](refactor/MIGRATIONS.md).
- Найденный баг фиксируется в [BUGS.md](refactor/BUGS.md) и закрывается вместе с падающим тестом.
- Счётчики предупреждений (`clippy-ratchet`, `eslint`) могут только убывать; `allow` не
  добавляются.
- После изменений в `crates/forma/src`, `Cargo.toml` или `Cargo.lock` пересобирать WASM
  (`npm run build:wasm`) перед финальным гейтом.
- В коммитах по `compute-cuda` без проверки на устройстве явно указывать, что на CUDA-железе
  запуск не выполнялся; сообщения коммитов — обычное предложение без префиксов типа.
