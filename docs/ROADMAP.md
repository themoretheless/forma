# Единый план развития Forma

Единственный файл планов по всему репозиторию (`Forma`, `Studio`, `Svet`, `GPU` и вычислительные
крейты) на 2026-11-26 (`29de6378`). Здесь собрана только незавершённая работа и действующие
правила выполнения; история выполненных этапов, базовые замеры, дефекты и изменения контрактов
хранятся в [ARCHITECTURE.md](refactor/ARCHITECTURE.md), [BASELINE.md](refactor/BASELINE.md),
[BUGS.md](refactor/BUGS.md), [INVENTORY.md](refactor/INVENTORY.md),
[MIGRATIONS.md](refactor/MIGRATIONS.md), [SNAPSHOTS.md](refactor/SNAPSHOTS.md),
[CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md),
[PRODUCTION_READINESS.md](../PRODUCTION_READINESS.md) и
[packages/svet/DEVELOPMENT.md](../packages/svet/DEVELOPMENT.md).

## Открытая работа

Открытых задач нет (`0`): все этапы плана развития `0.3.8` завершены и закрыты; текущие 9
последовательных замеров в [BASELINE.md](refactor/BASELINE.md) приняты как итоговая базовая линия
`0.3.8`, а команды их воспроизведения сохранены в [BASELINE.md](refactor/BASELINE.md) и
[CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md).

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
