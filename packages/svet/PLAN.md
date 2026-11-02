# План развития Svet

Статус: работа продолжается. План задаёт открытый объём; отдельные зелёные
проверки не означают завершения всего движка. История результатов и ограничения
проверок находятся в [DEVELOPMENT.md](DEVELOPMENT.md).

## Что осталось сделать — актуально на 2026-11-02

Это очередь открытых задач. Исторические результаты ниже не заменяют
повторную квалификацию текущих исходников и собранного пакета.

### Ближайшая очередь выполнения

1. **Довести HMR rollback и contract grammar.** Semantic-invalid inline
   contract admission проверен: Node6/6 и installed Chromium15/15; прежние
   DOM/state/cached factory сохраняются, valid update восстанавливает работу.
   Runtime bytes сверены с dist/package.
   [Отчёт](benchmarks/forma-hmr-validation-qualification-2026-11-02.json).
   Известные range/pattern с неверными аргументами теперь явно отклоняются:
   5 воспроизведённых отказов до правки, contract9/9 и full1351 после неё.
   Fresh installed package/GC и Chromium HMR15/15 прошли; общий matcher также
   исправляет потерю multiline pattern, проверенную installed AOT/runtime gate.
   Full1352/102 files; [отчёт](benchmarks/forma-pattern-parity-qualification-2026-11-02.json).
   Остались общая annotation grammar, политика unknown annotations и
   атомарный rollback AST/state/нескольких views. Staged construction и
   частичная DOM insertion теперь сохраняют предыдущий view при отказе;
   full1354, types/build, fresh installed package/GC и Chromium17/17 прошли.
   [Отчёт](benchmarks/forma-hmr-staging-qualification-2026-11-02.json).
   Все mounted views теперь подготавливаются перед retirement старых деревьев:
   отказ второго view сохраняет оба старых DOM; full1355 tests прошли.
   Fresh installed package/GC и Chromium18/18 прошли, все20 runtime files
   совпали с dist/package. [Отчёт](benchmarks/forma-hmr-multiview-qualification-2026-11-02.json).
   При preparation failure metadata/state/props/lets теперь восстанавливаются;
   старые computed handles остаются реактивными, candidate signals удаляются.
   Full1357, types/build, installed package/GC и Chromium19/19 прошли.
   [Отчёт](benchmarks/forma-hmr-document-qualification-2026-11-02.json).
   После начала commit rollback не гарантирован; in-place mutations и внешние
   побочные эффекты factory не восстанавливаются;
   connected callbacks могут видеть старые и кандидатные nodes одновременно.
   Reentrant dispose/unmount из old child cleanup на commit boundary проверены:
   candidate освобождается, retired view не публикуется; full1359 и Chromium21/21,
   types/build и installed package/GC проходят.
   [Отчёт](benchmarks/forma-hmr-commit-retire-qualification-2026-11-02.json).
   Произвольные cleanup exceptions и остальные reentrant commit callbacks открыты.
   Factory throw после создания same-copy Forma child теперь освобождает потерянный
   handle и helpers: full1360, types/build, installed package/GC и Chromium22/22.
   [Отчёт](benchmarks/forma-factory-qualification-2026-11-02.json).
   Constructor-owned allocations при props/locale/let metadata throw теперь
   освобождаются; external ViewModel signal сохраняется. Full1363, types/build,
   installed package/GC и Chromium25/25 прошли.
   [Отчёт](benchmarks/forma-constructor-qualification-2026-11-02.json).
   Construction между совместимыми copies одного JS realm проверен в обе
   стороны: installed Node2/2, paced Chromium27/27, full1363 и package/GC.
   [Отчёт](benchmarks/forma-construction-copies-qualification-2026-11-02.json).
   Unpaced browser run потерял watcher update5; отказ сохранён и не закрыт
   paced250ms прогоном. Трассируемый unpaced lifecycle27/27 и burst3×32 writes
   прошли с доставкой последней версии каждой серии; isolated Node6/6 тоже
   проходит. Это диагностическая матрица, причина прежнего отказа не доказана.
   [Расследование](benchmarks/forma-watcher-investigation-2026-11-02.json).
   Direct/atomic/chunked write matrix: Node18/18; Chromium9×32 writes
   (288 total) доставляют последнюю версию и завершаются graph0.
   [Отчёт](benchmarks/forma-watcher-write-matrix-qualification-2026-11-02.json).
   Причина прежнего пропуска не установлена; другие filesystems/editors/OS
   и воспроизведение отказа остаются открытыми.
   Другие realms/workers, incompatible versions и async factories открыты.
2. **Довести расследование HTTP external memory.** Получить финальный отчёт
   frozen 2h diagnostic, проверить provenance и accounting, объяснить пики.
   Его старый runtime не квалифицирует текущие исходники. После исправления
   повторить полный natural-GC gate текущего пакета без увеличения бюджетов.
3. **Закрыть матрицу cross-core lifecycle.** Расширить mixed-core scheduling,
   errors/cycles, browser GC и сохранённые handles; измерить overhead.
   Уже проверенные focused scenarios не считать общей гарантией.
4. **Снизить стоимость больших native forms.** Начать с per-control snapshots
   и edit-time rebuild; затем release A/A и A/B для 8/32/96/256 controls на
   спокойном host, включая полный кадр, latency и live/peak memory.
5. **Довести identity и native interaction.** Определить writable origins для
   derived collections, проверить transactions/reorder/replacement, несколько
   Scroll и окон, DPI, IME, focus, переключение GPU и device loss.
6. **Расширить среду исполнения.** Другие browser engines, async/Edge carriers,
   router bursts, rounded AA и physical GPU/backends, deployed SSR transports.
7. **Собрать единый release candidate.** Пройти все gates на одних исходниках
   и артефактах, clean install и remote CI; выпустить матрицу
   implemented / validated / partial / unsupported с evidence.

Для каждого пункта результатом должны быть исправление или явная граница
поддержки, regression, воспроизводимая команда и отчёт с hashes. Сборку пакета
и тесты, импортирующие dist, выполнять последовательно. Commit/push/release —
отдельным шагом по запросу. Детальная приёмка каждого направления — ниже.

### P1. Реактивность между независимыми cores

- [x] Реализовать базовый мост зависимостей: proxy signal в Rust-графе наблюдателя,
  forwarding effect в Rust-графе источника; JS передаёт уведомления и значения.
- [ ] Удалять динамические зависимости, освобождать обе стороны при dispose,
  reset и смене generation; исключить удержание cores через сильные ссылки.
- [ ] Проверить signal/computed, equality/lazy evaluation, batch, циклы,
  исключения, nested observers, устаревшие handles и GC независимых копий.

Базовый native proxy bridge реализован. Native batch удерживается во всех
затронутых cores до общей границы доставки; proxy writes сразу меняют native
state. Foreign boundaries computed-цепочки обновляются перед чтением кэша
внутри batch. Независимые копии проходят **22/22** probe cases.
Динамические зависимости, reset/dispose, reuse slots, equality/equals:false,
циклы и ошибки имеют regressions. Полная JS-серия: **1332 passed**, 100 files,
без expected failures. Types/package checks проходят.

- [x] Объединять уведомления target observer от нескольких foreign sources
  одного batch; бывший expected-failure regression теперь проходит обычно.
- [x] Проверить nested mixed-core batch, ошибки body/target, reset held target,
  новую generation в том же batch, свежесть computed/peek и ограничение
  feedback loop с последующим восстановлением.
- [x] Проверить three-core diamond с обоими порядками чтения, с batch и без
  него; объединить доставку для обычной записи с foreign subscribers.
- [x] Проверить запись из effect rerun и initial execution: очередь source-core
  flush имеет общую границу доставки, промежуточный diamond snapshot исключён.
- [x] Сохранять foreign computed dependencies после initial/later failure;
  target handling, same-value recovery, computed-chain и uncaught-error tests.
- [x] Проверить Chromium browser DOM failure/recovery на source и packed installed
  copies:5/5+5/5, error diamond, callback teardown и graph cleanup.
  [Отчёт](benchmarks/cross-core-error-browser-qualification-2026-11-02.json).
- [ ] Расширить произвольные mixed-core scheduling/diamond/cycle scenarios;
  browser qualification и измерения overhead.

- [x] Добавить dedicated explicit-teardown bridge GC gate: 9 сценариев ×8,
  440 teardown targets освобождены; 4 generic и 3 active-subscription controls
  остаются живыми до снятия ссылок и освобождаются после него. Gate встроен
  в package check для packed installed независимых копий.
  [Отчёт](benchmarks/cross-core-error-retention-qualification-2026-11-02.json).
- [ ] Проверить browser GC и долгоживущие/сохранённые пользовательские handles;
  forced-GC explicit-teardown результат не доказывает natural-GC plateau.

Приёмка всего этапа: расширенная матрица, отдельные bridge GC и browser
проверки подтверждены на текущем пакете. Обычный installed consumer/GC gate
не заменяет dedicated retention qualification для нового моста.
[Последняя квалификация](benchmarks/cross-core-error-qualification-2026-11-02.json).

### P1. Разобрать рост external memory в HTTP/SSR

- [ ] Сопоставить external/ArrayBuffers с очередями, сокетами, backpressure,
  WASM/native allocations и жизненным циклом запросов; найти источник пиков.
- [x] Добавить bounded diagnostics: server socket/response/queue counters,
  active resource kinds, V8 allocation metrics, global/window external peaks
  с одновременными counters и shutdown после socket.close. Smoke natural,
  forced и injected-failure прошли; причина external peaks ещё не установлена.
- [x] Проверять diagnostic socket/response accounting и shutdown в analyzer;
  regressions включены в Svet CI. Malformed schema и truncated JSON дают
  structured refusal; 9 unittest methods прошли. Remote CI пока не выполнен.
- [x] Добавить opt-in request core/memory FinalizationRegistry counters без
  сильного удержания targets; natural1936 requests, forced и disabled smoke,
  identity/gzip compatibility прошли. Это accounting callback lag, не live census.
  [Отчёт](benchmarks/http-core-lifetimes-qualification-2026-11-02.json).
- [x] Завершить frozen2h snapshot-Nv0ST2 с lifecycle tracing:
  [run metadata](benchmarks/http-core-traced-2h-run-2026-11-02.json).
  Подтвердить allocation/GC гипотезу длинным frozen run с lifecycle tracing;
  причина исторического 2h budget failure пока не доказана.
  Final:1144736 requests/7200025.895ms,0 request/cleanup failures,0 forcedGC;
  все4 бюджета соблюдены,external post-warmup peak growth49686991 bytes.
  Все381595 server sockets закрыты; runtime/source hashes совпали с manifest.
  Это старый runtime; текущий пакет не квалифицирован.
  [Финальный отчёт](benchmarks/http-core-traced-2h-qualification-2026-11-02.json).
- [x] Завершить frozen 30-minute diagnostic текущего installed runtime:
  [manifest](benchmarks/http-diagnostics-30m-manifest-2026-11-02.json),
  [run metadata](benchmarks/http-diagnostics-30m-run-2026-11-02.json).
  Это диагностический прогон, не замена 2-hour qualification.
  Final:359048 requests,0 failures/forcedGC,все server sockets закрыты;
  все4 memory budgets соблюдены,2h minimum не достигнут.
  [Final qualification](benchmarks/http-diagnostics-30m-qualification-2026-11-02.json).
- [ ] Воспроизвести и объяснить прежний отказ примерно на 24-й минуте.
- [ ] После исправления выполнить новый frozen-runtime natural-GC soak
  текущего пакета, со сверкой hashes и прежними бюджетами памяти.
  Запущен snapshot-RKk0T4 на consumer6fmUiE без lifetime tracing:
  [run metadata](benchmarks/http-current-runtime-natural-2h-run-2026-11-02.json).
  Квалификация pending,7200000ms minimum и16/64/64/16MiB бюджеты сохранены.
- [ ] Расширить проверки: gzip/codecs, slow clients, disconnect storms,
  socket saturation; отдельно deployed proxy/CDN, TLS и HTTP/2/3.

Последний двухчасовой прогон завершён: **1 289 120 запросов**, request/cleanup
failures — 0, nodes после завершения — 0. Рост post-warmup external peak
**72 409 329 bytes** превышает бюджет **67 108 864 bytes (64 MiB)**.
Heap/RSS/ArrayBuffers укладываются в бюджеты. Это требует расследования,
но само по себе не доказывает утечку. Прогон предшествует последней правке
reactivity.js и не квалифицирует новый dist. Повышение бюджета не является
исправлением. Приёмка: причина подтверждена измерениями, regression есть,
новый полный прогон проходит все четыре бюджета без forced GC.
[Отчёт](benchmarks/http-natural-latest-2h-qualification-2026-11-02.json).

### P2. Производительность native bindings и больших форм

Исходный allocation baseline: Svet256 edit240822218 requested bytes без paint.
Два scope-copy эксперимента не изменили counters и убраны; исходник восстановлен
byte-for-byte. Следующий шаг — измерить callsites глубоких __state/model clones,
сохранив props.__state и owned public snapshots.
[Отрицательный результат](benchmarks/native-scope-experiment-qualification-2026-11-02.json).

- [x] Убрать unconditional visual expand_sequence state clone: borrowed state
  до Let write, owned For locals; Svet256 edit requested bytes240.82→104.30MB,
  два release прогона с одинаковыми counters.160 lib/10 allocation/8 artifact,
  wasm32 build и actual Node WASM gate проходят.
  [Отчёт](benchmarks/native-state-cow-qualification-2026-11-02.json).
- [x] Убрать discarded visual AST clone и заимствовать definition nodes до
  supplied-content mutation: Svet256 edit104.30→103.14MB requested bytes,
  два совпадающих release allocation прогона;161 lib/10 allocation/8 artifact,
  wasm32 build и actual Node WASM проходят.
  [Отчёт](benchmarks/native-visual-shell-qualification-2026-11-02.json).
- [x] Передавать owned __state через visual instantiation с возвратом на Result
  success/error; не копировать всё дерево для root Frame measurement.
  Svet256 edit103.14→57.63MB requested bytes, два одинаковых allocation прогона;
  162 lib/10 allocation/8 artifact, wasm32 и actual Node snapshot2/2 проходят.
  ABI gates добавлены в studio CI; remote CI не выполнялся.
  [Отчёт](benchmarks/native-snapshot-transfer-qualification-2026-11-02.json).
- [x] Передавать expanded flat Frame children вместо clone и использовать
  snapshot transfer в fallback: Frame Svet256 edit103.23→57.38MB requested,
  два одинаковых allocation прогона; Column counters не изменились.
  163 lib/10 allocation/8 artifact и actual Node snapshot3/3 проходят.
  [Отчёт](benchmarks/native-flat-transfer-qualification-2026-11-02.json).
- [x] Передавать resolved collection path отдельно от полного lexical frame:
  --lexical Svet256 edit599.37→509.11MB requested bytes(-15.1%), два одинаковых
  allocation прогона; ordinary Column counters не изменились.164 lib/10
  allocation/8 artifact, wasm32 и actual Node alias/snapshot4/4 проходят.
  [Отчёт](benchmarks/native-lexical-frame-qualification-2026-11-02.json).
- [x] Заимствовать начальный Let replay state и передавать evaluated locals:
  lexical Svet256 edit509.11→419.40MB requested bytes(-17.6%), два одинаковых
  allocation прогона; ordinary counters неизменны.164 lib/10 allocation/8
  artifact, wasm32 и actual Node single/chained alias5/5 проходят.
  [Отчёт](benchmarks/native-let-replay-qualification-2026-11-02.json).
- [x] Не копировать captured locals/parent values, заменяемые replayed lexicals:
  lexical Svet256 edit419.40→374.63MB requested bytes(-10.7%), два одинаковых
  allocation прогона; ordinary counters неизменны.164 lib/10 allocation/8
  artifact, wasm32 и actual Node alias/snapshot5/5 проходят.
  [Отчёт](benchmarks/native-local-overlay-qualification-2026-11-02.json).
- [x] Убрать duplicate private frame locals, уже сохранённые в __lexicals:
  lexical Svet256 edit374.63→284.71MB requested bytes(-24.0%), два одинаковых
  прогона; ordinary counters неизменны.165 lib/10 allocation/8 artifact,
  wasm32/actual Node5/5 и old/new WASM artifact comparison4/4 проходят.
  256-control JSON35.65→28.62MB; preceding local reader принимает новый формат.
  [Отчёт](benchmarks/native-frame-dedup-qualification-2026-11-02.json).
- [x] Убирать root replay metadata только при точной копии в retained frames:
  owned256-control artifact181.09→135.62MB Rust allocator bytes(-25.1%),
  Drop delta0 во всех samples; --artifact-retention измеряет отдельно от edit.
  JSON28.62→21.52MB.166 lib/10 allocation/8 artifact, actual Node5/5 и
  old/new comparison4/4 проходят. Edit requested counters не изменились.
  [Отчёт](benchmarks/native-replay-transport-qualification-2026-11-02.json).
- [ ] Приоритет: снизить оставшиеся Let replay/model copies в lexical forms;
  284MB requested/edit при256 controls ещё не означает interactive readiness.
- [ ] Уменьшить оставшиеся per-control snapshots модели и scopes, повторную scene expansion
  в public Document sync и edit-time rebuild; проверить сложные key expressions.
- [ ] Замерить Let replay и sharing между frames; сохранить независимость
  snapshots и доступ к props.__state.
- [ ] Снять новый release baseline для 8/32/96/256 controls: legacy/Svet,
  idle/edit, A/A и A/B на спокойном host; отдельно render и presentation.
- [ ] Измерить latency p50/p95/p99, live/peak memory и RSS; cumulative requested
  allocator bytes публиковать отдельно. Повторить на другом оборудовании.
- [x] Убрать discarded supersampling из radius=0 path и восстановить safe
  interior fast path без reveal. CPU/reference и hardware GPU regressions прошли.
  [Отчёт](benchmarks/native-square-cache-qualification-2026-11-02.json).
- [ ] Измерить release стоимость точного rectangle/border AA и cache build
  после удаления лишней работы на спокойном host; FPS/latency не подтверждены.

Уже сделаны owned transfers layout/position/visual/state, lazy staging и
transaction-local key index. Idle96 достиг 1331 allocations; прежний edit256
около 238.43 MB requested bytes остаётся историческим измерением.
Приёмка: актуальные source-bound измерения, корректность bindings/scopes,
allocation budgets, native/wasm checks; готовность больших форм подтверждается
полным кадром и согласованным целевым latency/memory бюджетом.

### P2. Identity, транзакции и lexical scopes

- [ ] Определить безопасный writable-origin контракт для derived/filter/map
  collections; неподдержанные неоднозначные записи явно отклонять.
- [ ] Проверить замену компонента с тем же key и перенос pending events через
  замену документа: без потери, повторной доставки и записи в чужой instance.
- [ ] Расширить model/widget conflict matrix, deep/computed scopes и malformed
  metadata; ошибки и повторные отказы должны сохранять ввод и атомарность.
- [ ] Подтвердить compiler/AOT/browser parity для source Let/For/If/Match,
  shadowing, alias chains и reorder/removal/duplicate identities.

Прямые root-backed и parent-member aliases, lexical frames и source Let уже
имеют focused coverage; общая derived/deep matrix остаётся открытой.
Приёмка: source-level regressions для каждой границы, fresh/cached maps,
rollback/retry, однократное принятие события и пустой graph после teardown.

### P2. Native lifecycle, окна и Scroll

- [ ] Проверить несколько форм/окон, callbacks/scopes, close/reload/document
  replacement и отсутствие оставшихся jobs/resources.
- [ ] Пройти реальные OS resize, несколько мониторов/DPI, IME/focus/input,
  CPU↔GPU switching и device-loss recovery с видимым кадром.
- [x] Сохранить explicit-height пустого layout в sole-root Scroll retained
  `.ui` Runtime: legacy/Svet rebuild, offset preservation/shrink и layout revision.
  [Отчёт](benchmarks/native-logical-scroll-qualification-2026-11-02.json).
- [x] Передавать logical bounds через публичный Rust SceneArtifact и
  Runtime::from_scene_artifact; Form::sync использует тот же артефакт.
  [Отчёт](benchmarks/native-scene-artifact-qualification-2026-11-02.json).
- [x] Добавить versioned JSON SceneArtifact с lossless typed values, atomic load
  и WASM compile/load API; actual generated NodeJS WASM execution прошёл.
  [Отчёт](benchmarks/native-scene-json-qualification-2026-11-02.json).
- [x] Проверять mapping count и key/order против загруженных controls до
  замены Runtime; native и actual Node WASM refusal/recovery tests прошли.
  [Отчёт](benchmarks/native-scene-mapping-qualification-2026-11-02.json).
- [x] Проверить browser ESM/WASM JSON artifact в Chromium: 5/5 scenarios,
  clipping/pixel-change checks и CPU scales1/1.25/2.
  [Отчёт](benchmarks/native-scene-browser-qualification-2026-11-02.json).
- [x] Повторить current native WASM Chromium compile/input/CPU raster slice
  после ownership/metadata правок:8/8,single/chained Unicode alias commit,
  custom props.__state update,clipping/refusal recovery,scales1/1.25/2.
  [Отчёт](benchmarks/native-current-browser-qualification-2026-11-02.json).
- [ ] Расширить artifact browser matrix на другие engines/browser GPU;
  legacy raw markup остаётся control-only, logical metadata требует artifact.
- [x] Сохранить leading edge oversized content в sole-root Scroll: empty
  Row/Column/Stack/Frame/Grid и TextInput, полный192x300 offset и возврат0.
  Native, actual Node WASM и existing hardware GPU regressions прошли.
  [Отчёт](benchmarks/native-scroll-horizontal-qualification-2026-11-02.json).
- [ ] Довести вложенные/несколько Scroll viewports, общую overflow/empty-layout
  matrix и browser interaction; проверить
  clipping, hit testing и offset clamp.

Sole-root Scroll и focused resize/scroll recovery проверены; это не полная
оконная квалификация. Приёмка: реальные окна, сохранённое состояние,
корректные кадры/ввод и освобождение ресурсов после каждого сценария.

### P3. Browser, router и async lifecycle

- [ ] Пройти непрерывный high-rate history burst; paced burst не заменяет его.
- [ ] Проверить реальный Vite HMR и production AOT: повторные mount/unmount,
  callbacks, ownership, pending async work, ошибки и восстановление.
- [ ] Расширить независимые package copies, browser/Edge async carriers,
  другие браузеры и deployed providers; явно описать unsupported границы.

Приёмка: автоматические browser regressions плюс наблюдаемая работа
приложения, отсутствие stale callbacks и остаточных graphs/listeners/tasks.

### P3. GPU и визуальная корректность

- [ ] Расширить fractional AA на rounded borders и frame masks.
- [ ] Проверить projection/picking/clipping/depth независимыми reference
  comparisons, сложные сцены и сочетания прозрачности.
- [ ] Проверить другие physical GPU/backends и восстановление после отказов.

На Apple M4 Max/Metal квадратные rectangles/borders прошли 72 CPU/GPU cases
с max difference 1/255; это не покрывает rounded geometry и другие backends.
Приёмка: сохранённые source-bound pixel artifacts, явные tolerances,
независимые эталоны и hardware matrix без скрытых исключений.

### P4. Итоговая квалификация и доставка

- [ ] Собрать единую матрицу implemented / validated / partial / unsupported
  с командами, версиями среды, hashes и ссылками на evidence.
- [ ] Запустить полные JS/types/package/installed-GC, Rust/native/wasm,
  hardware GPU, browser/AOT и долговременные SSR gates на одном кандидате.
- [ ] Проверить remote CI, чистую установку и provenance Rust→WASM→package;
  повторить критические сценарии на другом оборудовании.
- [ ] Отдельно подготовить commit/merge/push/release, когда это будет запрошено.

Готовность подтверждается только всей согласованной матрицей текущего
кандидата. Частные зелёные тесты не закрывают P1–P4 и не означают «до предела».

Рекомендуемый порядок: cross-core bridge → external-memory investigation →
native snapshots/performance → identity/lifecycle → browser/GPU environments →
единый qualification candidate. Длительные прогоны запускать на замороженном
кандидате после зависимых правок, сохраняя manifest и финальный отчёт.

## История частных результатов

Release retained Runtime измерен на8/32/96/256 editors, legacy/Svet, два
последовательных прогона с совпавшими allocation counters. Svet256: idle
278501 requested bytes, edit377214957 bytes; edit median93–95ms в текущем
неизолированном окружении, без paint/presentation. Следующий приоритет —
per-control model snapshots и edit-time rebuild; готовность больших форм
не подтверждена. [Отчёт](benchmarks/native-retained-runtime-qualification-2026-11-02.json).

Owned normalization убрала повторные копии промежуточных layout trees:
Svet256 edit377.21→330.37MB requested bytes (−12.4%). Timing нестабилен;
ускорение не подтверждено. [Отчёт](benchmarks/native-owned-layout-qualification-2026-11-02.json).

Позиционирование теперь потребляет expanded children: Svet256 edit
330.37→239.39MB requested bytes (−27.5%), median57.4–57.7ms в текущем
прогоне без paint. Это ещё не interactive readiness.
[Отчёт](benchmarks/native-owned-position-qualification-2026-11-02.json).

Precomputed visual trees теперь тоже передаются во владение: Svet256 edit
239.39→238.43MB requested bytes (−0.40%). Выигрыш небольшой; основной
открытый расход — snapshots модели на каждый control.
[Отчёт](benchmarks/native-owned-visual-qualification-2026-11-02.json).

Scheduler больше не копирует неизменённую модель на idle: Svet96 сравнялся
с legacy —1331 аллокация. Исправлены уведомления при добавлении/удалении
пустых scopes; pending render retry сохраняется.
[Отчёт](benchmarks/native-idle-graph-qualification-2026-11-02.json).

Вложенные root-backed пути через индексы списков поддержаны в чтении/записи
и IR Document bindings; reorder сохраняет stable-key адресацию. Это ещё не
поддержка nested For через parent locals и не compiler/AOT qualification.
[Отчёт](benchmarks/native-nested-paths-qualification-2026-11-02.json).

Nested For через parent local теперь разрешает root-backed путь по stable
parent key. Реальный .ui проверен на parent/child reorder, удаление родителя,
дубликаты и fresh/cached maps. Derived collections, одинаковые shadowed имена
родителя и полная computed/deep matrix остаются открытыми.
[Отчёт](benchmarks/native-nested-for-qualification-2026-11-02.json).

Соседние nested For с одинаковым item-name теперь сохраняют lexical parent
context: проверены12 reorder/removal/duplicate сценариев и три уровня item.
Открыты nonadjacent shadowing, общий scope chain, стоимость parent snapshots
и computed/derived collections.
[Отчёт](benchmarks/native-shadow-parent-qualification-2026-11-02.json).

Nonadjacent item→branch→item проверен на reorder и fresh/cached maps.
Полный lexical environment всё ещё нужен для поздних ссылок на более ранние
locals через несколько shadowings, а также computed keys; не считать scope
chain завершённым. [Отчёт](benchmarks/native-nonadjacent-shadow-qualification-2026-11-02.json).

Binding identity теперь использует плоские lexical frames с declaration-time
parent indexes; рекурсивные whole-state parent snapshots больше не создаются.
Поздняя ссылка на branch после item-shadow + computed key + reorder проверены.
Открыты Let/root-name shadowing, shared locals/snapshots, deep/error matrix
и derived collections. [Отчёт](benchmarks/native-lexical-frames-qualification-2026-11-02.json).

Let-значения теперь явно сохранены в lexical frames и имеют приоритет над
одноимённым root-полем; compound key + reorder проверены fresh/cached в IR.
Открыты динамическая переоценка Let, Let-derived aliases и полная compiler/
browser/AOT matrix. [Отчёт](benchmarks/native-let-scopes-qualification-2026-11-02.json).

Let replay теперь хранит выражения и порядок объявлений; текущая модель и
prior Let results участвуют в восстановлении key. Constant/dynamic prefix→suffix
и смена key identity проверены fresh/cached в IR. Открыты replay allocations,
Let-derived aliases и полная error/deep/compiler/browser matrix.
[Отчёт](benchmarks/native-dynamic-let-qualification-2026-11-02.json).

Let error/retry regression: поздний отказ откатывает ранние staged writes,
два повторных отказа сохраняют ввод; исправление принимает обе записи один
раз и не переигрывает widgets поверх новой модели. Проверено fresh/cached в IR.
[Отчёт](benchmarks/native-let-retry-qualification-2026-11-02.json).

Let replay переиспользует рабочую модель между steps с восстановлением
overlays; полная копия теперь одна на replay frame. Измерение latency/bytes
и sharing между frames остаются открытыми.
[Отчёт](benchmarks/native-let-replay-reuse-qualification-2026-11-02.json).

Lexical-frame routing валидирует индекс/item/parents и limit64; повреждённые
данные отказывают без legacy fallback. Отрицательный active index проверен
с атомарным rollback и последующим retry ввода. Полная malformed matrix открыта.
[Отчёт](benchmarks/native-frame-validation-qualification-2026-11-02.json).

Прямые Let collection aliases теперь сохраняют symbolic model origin:
rows=state.items→chosen=rows, For chosen/state.chosen, root-name shadow и
reorder проверены fresh/cached в IR. Совпадение значений copied list не даёт
права записи в модель. Общие computed/filter/map collections ещё открыты.
[Отчёт](benchmarks/native-let-alias-qualification-2026-11-02.json).

Parent-member Let aliases (`rows=outer.items`) проверены в расширенной
матрице24 сценария: direct/alias, shadow names, fresh/cached, reorder/removal/
duplicate parent. Let wrapper добавлен в IR; source-syntax/browser/performance
qualification остаётся открытой. [Отчёт](benchmarks/native-parent-alias-qualification-2026-11-02.json).

Исходный `.ui` Let внутри For теперь виден следующим siblings; итерации
изолированы. Все24 alias/reorder/error сценария переведены на source syntax
без вставки IR; отдельный scope-leak/key-stability тест проходит. Performance
For+Let, browser/AOT/window matrices остаются открытыми.
[Отчёт](benchmarks/native-source-let-qualification-2026-11-02.json).

Последовательные scopes унифицированы для containers/For/If/Match/Let.
Исходный If/else с зависимыми Let и переключениями проверен: locals видны
внутри ветки и не выходят наружу. Открыты сложная Match/deep matrix и
release performance. [Отчёт](benchmarks/native-sequence-scopes-qualification-2026-11-02.json).

Source Match guard + Let aliases + different branch bindings проверены:
старый widget не пишет в новую ветку, при возврате ключ восстанавливается,
locals не выходят наружу. Cached branch-switch/error и deep matrix открыты.
[Отчёт](benchmarks/native-match-scopes-qualification-2026-11-02.json).

Исправлен reproduced Match-switch+render-error bug: recovery использует
current structural binding map без visual/layout, не старую карту ветки.
Legacy/Svet retained Runtime проверены до/после; structural expansion error
теперь отказывает без stale fallback. Полная selection/error matrix открыта.
[Отчёт](benchmarks/native-branch-recovery-qualification-2026-11-02.json).

Changed-model preflight теперь только structural binding expansion; visual
render выполняется после bindings. Repair98-control формы проверен с budget
<450000 аллокаций (353598 наблюдено). Release сравнение и peak memory открыты.
[Отчёт](benchmarks/native-structural-preflight-qualification-2026-11-02.json).

Structural/rendered maps совпали в18 layout/branch/reorder сценариях по
node/key/state/event_args. Исправлен negative-y Scroll из-за несовместимого
precomputed kind. Новый открытый дефект: Scroll viewport props (width/height)
не представимы текущей сериализацией runtime grammar; нужно исправить, не
считать Scroll matrix полной. [Отчёт](benchmarks/native-binding-map-parity-qualification-2026-11-02.json).

Root Scroll width/height теперь lowering в Frame padding: numeric/percent,
root padding, сохранение внешних размеров, scroll clamp и hit testing проверены.
Предыдущий serialization defect закрыт для sole root Scroll внутри frame bounds.
Multiple/nested viewports, empty content/resize и pixel/window matrix открыты.
[Отчёт](benchmarks/native-scroll-viewport-qualification-2026-11-02.json).

Исправлена layout_revision при viewport-only resize; retained legacy/Svet
проверены на сохранение/clamp offset, empty items→0 controls/offset и idle.
OS-window/multi-monitor resize и viewport pixel matrix ещё открыты.
[Отчёт](benchmarks/native-scroll-resize-qualification-2026-11-02.json).

Physical Metal Scroll reference:9 кадров (DPI1/1.25/2 × offsets0/32/64),
exact RGBA на96.875% пикселей; outside viewport проверен полностью. PNG
сохранены/просмотрен representative frame; GPU suite4/4. Scrollbar, другие
backends и window resize matrix открыты.
[Отчёт](benchmarks/native-scroll-pixels-qualification-2026-11-02.json).

Fractional viewport63×61 at32.5/33.5 проверен на Metal:9 кадров по
pixel-center clip контракту, все outside pixels exact; AA/seam/scrollbar только
внутри исключены. GPU5/5; это не area-AA/cross-backend/window qualification.
[Отчёт](benchmarks/native-fractional-scroll-qualification-2026-11-02.json).

Приоритетный воспроизведённый дефект: CPU/GPU fractional AA parity —
3/18 кадров расходятся до59/255 на corners/seams. Новый regression падает;
hardware suite5 passed/1 failed. Нужно унифицировать precise rectangle
coverage/compositing, сохранив качество GPU. Предыдущие crop proofs не
закрывают эту область. [Данные](benchmarks/native-scroll-renderer-parity-qualification-2026-11-02.json).

Fractional AA defect закрыт для borderless radius0 rectangles: CPU/GPU exact
pixel-area coverage,18/18 кадров max1/255 (было59). Независимые corner/seam
эталоны проходят; GPU6/6. Rounded/bordered AA, frame masks и performance
стоимость sparse edges ещё открыты.
[Отчёт](benchmarks/native-exact-rect-qualification-2026-11-02.json).

Translucent rectangle parity:18 alpha кадров exact, совокупно36/36 CPU/GPU
max1/255; независимые alpha corner/seam эталоны проходят, Metal6/6.
Rounded/bordered/frame-mask alpha matrix открыта.
[Отчёт](benchmarks/native-alpha-parity-qualification-2026-11-02.json).

Square border AA теперь exact outer-minus-inset area с translucent source-over.
До32/255 расхождения исправлены:72/72 кадров max1/255, thin1.25 и covering40
borders проверены, Metal6/6. Rounded border AA и performance остаются открытыми.
[Отчёт](benchmarks/native-exact-border-qualification-2026-11-02.json).

P1 cross-core refreshed:11/13; foreign invalidation и nested observer attribution
воспроизведены на current independent builds. Требуется active-domain routing
и Rust proxy-node bridge, с generation/lifetime, dynamic retirement, teardown,
computed equality/lazy, batch и cycle gates; host — только transport.
[Отчёт](benchmarks/cross-core-current-qualification-2026-11-02.json).

Cross-core attribution исправлена: paused outer не получает foreign read.
JS1301/99 + types/package/installed GC проходят; probe11/13 всё ещё открыт
из-за target invalidation (native proxy bridge не реализован).
[Отчёт](benchmarks/cross-core-domain-routing-qualification-2026-11-02.json).

Frozen latest-at-launch2h HTTP завершён:1289120 requests, failures0, hashes
20/20 verified. External peak growth72.41MB >64MiB budget; остальные3 budgets
прошли. Нужен разбор external memory, budget не повышать. Текущий dist уже
отличается reactivity.js; новый runtime этим не квалифицирован.
[Final](benchmarks/http-natural-latest-2h-qualification-2026-11-02.json).

Последний native checkpoint: lib 158/158, allocation 8/8, Metal GPU 6/6,
native/wasm32 checks. Это ограниченная локальная проверка, а не полная готовность.
См. [последний отчёт](benchmarks/native-scope-cow-qualification-2026-11-02.json).

## Рабочий план: что осталось сделать — 2026-11-02

Этот раздел задаёт актуальную последовательность. Ниже сохранены подробности
и история проверок; старые результаты не подтверждают последующие редакции.
Незакрытые пункты отмечены `[ ]`. Готовый owned Rust context — основа интеграции,
а не готовый native backend.

### Этап 1. Подключить Svet к настоящим native bindings Forma

- [x] Добавить native dependency `svet-wasm` в `vector-ui` и owned Context
  на экземпляр Form; сохранить обычный и wasm backend.
- [x] Направить изменения Property/Field через signal/effect графа Svet:
  постановка задач, batch dedup, drain и повторное tracking перед обновлением Runtime.
- [ ] Сохранить two-way input, context replacement, reentrant notifications
  и pending updates при переключении backend; исключить потерю обновлений.
- [x] Реализовать освобождение bindings и графа при replace/drop, включая
  сохранённые внешние Control handles; stale jobs не должны затрагивать новые поля.
- [ ] Проверить реальные значения и отображение Form до/после обновления,
  изоляцию двух форм, переключение backend и graph/edges/jobs после teardown.

Приёмка: обновление native Form причинно проходит через Svet graph, а тесты
проверяют фактический Runtime и освобождение ресурсов.

Реализован opt-in `Form::set_svet_bindings(true)`. Local native lib suite:
117/117; wasm32 check прошёл. Проверены Runtime value, two-way input, batch
dedup, context replacement/reentrant wake, backend switching, stale queued
binding replacement и teardown с сохранённым Control. Document render reentry,
реальные окна и полный Studio end-to-end остаются открытыми.
[Отчёт](benchmarks/native-form-bindings-qualification-2026-11-02.json).

### Этап 2. Довести native Studio и compiler до end-to-end

- [x] Подключить Svet к пути запуска `studio:native -- --svet`:
  `vector-ui/src/main.rs`, retained Document/Runtime и render scheduling.
  Интеграция typed Form сама по себе этот путь не закрывает.
- [ ] Определить ownership callbacks/values/component scopes, wake/redraw,
  замену документа, закрытие окна и несколько окон на одном потоке.
- [ ] Пройти запуск → input → изменение модели → видимый кадр → teardown
  на настоящем native backend с телеметрией и сохранённым результатом.
- [ ] Проверить production Forma/AOT и реальный Vite HMR в браузере:
  несколько mounts, последний unmount, dispose, смена AST и сохранение состояния.

Реализован retained Document scheduler: по signal на поле state, structural
signal и render effect; graph сохраняется при пересборке Runtime. CLI, F7
и hot reload выбирают этот scheduler. Native lib119/119, native-bin check
и wasm32 check проверяются отдельно; реальный оконный end-to-end ещё открыт.
[Отчёт](benchmarks/native-document-scheduling-qualification-2026-11-02.json).

Native window на Apple M4 Max/Metal/Retina2: запуск, ввод, F7 GPU→CPU→GPU,
зависимый текст, hot reload с сохранением state и следующий ввод прошли;
закрытие окна завершило процесс. Исправлены contentsScale Metal-слоя и
перекрытие GPU сохранённым softbuffer-кадром. Три аппаратных GPU-теста прошли.
Это один проверенный сценарий, не полная matrix окон/renderer/IME/ресурсов.
[Отчёт и снимки](benchmarks/native-window-qualification-2026-11-02.json).

Исправлено восстановление после state-dependent render error: последнее
успешное binding mapping принимает исправляющий ввод; pending работа и актуальное
значение toggle сохраняются. Библиотечные120/120, четыре legacy/Svet/switch
сценария, wasm32/native build и hardware GPU3/3 прошли. В живом окне ошибка
остаётся видимой до исправления, затем summary догоняет editor без перезапуска.
Collection reordering при отказах и полная transactional error matrix ещё открыты.
[Отчёт](benchmarks/native-binding-recovery-qualification-2026-11-02.json).

Bindings теперь атомарны при ошибке metadata: staging root scope коммитится
только после успешной обработки всех bindings. Повтор после исправления принимает
сохранённый ввод и одну неприменённую активацию ровно один раз. Проверены обычная
и cached-scene синхронизация, пустые states, legacy/Svet и структурные limits
64/16384 до рекурсивного копирования. Lib123/123, native/wasm32 checks прошли.
Очередь нескольких новых активаций, identity коллекций при reorder, вложенные
Value payloads и цена staging на больших моделях остаются открытыми.
[Отчёт](benchmarks/native-binding-transaction-qualification-2026-11-02.json).

Добавлена упорядоченная очередь активаций retained Runtime: keyboard/pointer/
direct события не теряются при ошибке bindings, весь пакет применяется атомарно.
Проверены 1–3 toggle, порядок selected/toggle на общей модели, повторные отказы,
переполнение4096 и явная отмена через `cancel_ui_binding_activations`.
Lib126/126, native/wasm32 checks прошли. При overflow синхронизация отказывает
до cancel/reload, а не частично применяет prefix. Перенос событий через reload/
reorder, журнал прошлых widget values и latency у лимита ещё открыты.
[Отчёт](benchmarks/native-activation-queue-qualification-2026-11-02.json).

Исправлена маршрутизация bindings при reorder: expanded controls сопоставляются
с живыми widgets по ключам, а root-backed For items заново находятся по stable
key (включая сохранённые key expression/scope при ошибке шаблона). Проверены
активации, повторные toggle и editors через reorder/remove/insert, fresh/cached
maps, отказ на duplicate keys и реальный retained Runtime recovery с key от props.
Lib130/130 и native/wasm32 checks прошли. Nested/derived collections, перенос
через замену документа, конфликт model/widget и стоимость поиска ещё открыты.
[Отчёт](benchmarks/native-binding-key-qualification-2026-11-02.json).

Исправлен конфликт model/widget: неизменённые text/range widgets больше не
затирают новое значение модели. Сравнение идёт с последним принятым snapshot;
неприменённое изменение widget имеет приоритет на своём binding. Snapshot
подтверждается только после успешного атомарного commit (не после отказа).
Проверены повторные sync, range keyboard input, concurrent same-field значение,
reorder с обновлённым нетронутым item и исправление render error из модели.
Lib132/132, allocation5/5, native/wasm32 checks прошли. Это value-snapshot policy,
а не журнал ревизий ввода; large-model performance и прочие conflict cases открыты.
[Отчёт](benchmarks/native-widget-conflict-qualification-2026-11-02.json).

Измерен public Document binding sync в release на8/32/96 editors. Попытка
кеша индексов не дала устойчивого выигрыша и удалена. Оставлено заимствование
данных вместо лишних list clones: на96 editors число аллокаций снизилось≈5%,
сумма requested bytes≈5.6% (повторные счётчики совпали). Время не улучшилось
надёжно; повтор C2 показал +5–8% и требует разбора на спокойном host.
135 lib tests, allocation5/5, native/wasm32 checks прошли. Добавлен воспроизводимый
`binding_sync_bench` и архив сравниваемых исходников. Квалификация производительности
открыта: остаётся≈110 MB суммарных запросов аллокатора на одну sync96 editors.
Следующий приоритет — разделить стоимость preflight/render и bindings, уменьшить
копирование root state/scopes, затем повторить latency/peak-memory на приложении.
[Отчёт](benchmarks/native-binding-copy-qualification-2026-11-02.json).

Разделены render_scene и public sync: построение сцены давало88–93% requested
bytes. Убраны глубокие копии, чьи props/children сразу заменялись, и ненужная
копия measuring root (legacy Frame→Column conversion сохранена). На96 editors
render bytes97.1→83.8MB (-13.7%), полная sync110.3→96.9MB (-12.1%), allocation
calls874257→774668 (-11.4%); повторы совпали по счётчикам. Latency всё ещё
неустойчива, FPS выигрыш не подтверждён. Lib135/135, allocation5/5, Metal GPU3/3,
native/wasm32 checks прошли. Тяжёлые state/scope snapshots остаются открытыми.
[Отчёт](benchmarks/native-render-phases-qualification-2026-11-02.json).

Убрана вторая полная копия instance.props при instantiate: восстанавливаются
только явные ключи, перекрытые выбранной группой. Приоритет props и доступ
к props.__state проверены. На96 editors полная sync96.94→90.39MB requested
bytes (-6.8%), allocation calls774668→725708 (-6.3%). Оба локальных timing
повтора быстрее, но app FPS/p95 ещё не квалифицированы. Lib135/135,
allocation5/5, Metal GPU3/3, native/wasm32 checks прошли.
[Отчёт](benchmarks/native-instance-overrides-qualification-2026-11-02.json).

Второй frozen HTTP soak LPJBKt завершён:918440 requests/7200008ms,
request/cleanup failures0, graph0, forcedGC0, все четыре memory budgets прошли.
Хэши harness/fixture/runtime20 независимо сверены. Latest dist отличается
в advanced/forma/WASM/embedded binary; его квалификация этим не закрыта.
Запущен отдельный latest2h natural прогон09qhPH (20/20 files совпали с dist).
[Final LPJBKt](benchmarks/http-natural-current-2h-qualification-2026-11-02.json),
[новый запуск](benchmarks/http-natural-latest-run-2026-11-02.json).
Уточнение benchmark-контекста: поздние copy/render замеры выполнялись после
завершения HTTP measurements; старый tool handle не доказывал активную нагрузку.
[Коррекция](benchmarks/native-benchmark-http-overlap-correction-2026-11-02.json).

Наследуемые state/props при scene expansion теперь borrowed до локальной записи
(Cow); Let/node-local overlay отделяются, публичные snapshots остаются owned.
Проверены вложенные scopes, sibling isolation и сохранность старого snapshot
после изменения модели. На96 editors sync requested bytes90.39→83.84MB (-7.2%),
allocation calls725708→677030 (-6.7%). Lib136/136, allocation5/5, Metal GPU3/3,
native/wasm32 checks прошли. Эти замеры сопровождались подтверждённым active
HTTP churn; latency не квалифицирована. Выходные snapshots/For locals всё ещё
копируют модель, large-model readiness остаётся открытой.
[Отчёт](benchmarks/native-scope-cow-qualification-2026-11-02.json).

### Этап 3. Закрыть оставшиеся дефекты реактивности и lifecycle

- [ ] Закрыть failing cross-copy dependency между разными cores (probe 11/12):
  dynamic dependencies, batches, cycles, disposal/reset и coherent observers.
  Если сценарий исключается из контракта, явно зафиксировать отказ и ограничение.
- [ ] Разобрать непрерывный high-rate history burst с native-only контролем;
  paced pass не считать закрытием непрерывного сценария. Проверить конкурирующие routers.
- [ ] Проверить browser/Edge async carriers: nested/concurrent await,
  rejection/abort, streaming, explicit capture и поздние callbacks после teardown.
- [ ] Дополнить lifecycle проверки native forms APIs, retained DOM/receiver,
  callbacks и нескольких одновременно живых cores/roots.

### Этап 4. Завершить память, SSR и транспорт

- [x] Получить и разобрать final report второго frozen HTTP soak; сверить
  manifest/hashes и четыре memory budgets. Не считать progress-файл финалом.
- [ ] Повторить длительный прогон на окончательном runtime: LPJBKt предшествует
  route-table/AOT/native ABI исправлениям и не квалифицирует их.
- [ ] Установить исходную причину отказа прежнего ~24-минутного прогона.
- [ ] Провести длительную нагрузку с обоими codecs, gzip, slow clients,
  крупными patches, disconnect/abort и насыщением сокетов; проверить queues/recovery.
- [ ] Раздельно оценить heap/RSS/external/ArrayBuffers, WASM arena и GPU;
  исследовать рост/retainers и стоимость ограничения арены относительно baseline.
- [ ] Проверить конкретный deployment: proxy/CDN, TLS/HTTP2/3, cancel/FIN,
  mixed server/client versions, malformed frames и отказ до DOM mutation.

### Этап 5. Расширить браузеры, Edge и аппаратный renderer

- [ ] Дополнить матрицу другими браузерами и deployed Edge providers,
  natural GC и stable Miniflare после beta.
- [ ] На физических GPU/backends проверить repeated device loss/recovery,
  cancellation, resize/DPR, shared device, resident buffers и renderer teardown.
- [ ] Сравнить projection/picking/clipping/depth с эталонами для перекрытий,
  невыпуклой геометрии и граничных координат; сохранить визуальные результаты.

### Этап 6. Измерить приложения и подготовить проверяемую доставку

- [ ] Выполнить A/A и A/B реальных приложений: startup, latency/p95,
  frame time, long tasks, transfer bytes, queue/compute/readback и память.
  Повторить без конкурирующей нагрузки и на другом оборудовании.
- [ ] Собрать единую матрицу implemented/validated/partial/unsupported
  по функциям и средам, со ссылками на актуальные raw evidence.
- [ ] Проверить remote CI, чистую установку, archive/exports/types и цепочку
  Rust → embedded WASM → installed package; закрепить обязательные gates.
- [ ] Актуализировать README/DEVELOPMENT по окончательному поведению.
- [ ] Commit/merge/push/release выполнить отдельным запрошенным этапом,
  после проверки состава изменений и сохранения посторонней работы.

Общее правило приёмки: воспроизводимый сценарий → исправление/регрессия →
проверка реального backend или установленного пакета → raw evidence с hashes
и ограничениями. Полный suite повторять после затронувших runtime изменений;
документационные правки проверять отдельно. Отдельный зелёный тест, микробенчмарк
или forced-GC прогон не закрывает весь этап.

## Ближайший план оставшихся работ — 2026-11-02

Этот раздел обновляет приоритеты очереди ниже. Галочка означает проверенный
ограниченный результат; P1–P4 в целом остаются открытыми.

Подтверждённая база: 1297 JS-тестов / 99 файлов, TypeScript, installed-package
gate с Edge inspector GC и HTTP actions 4/4. Независимые browser bundles прошли
9/9 проверок. Явный синхронный выбор core согласован между совместимыми копиями;
captured callback работает после завершения child carrier, пока жив его owner.
[Последняя квалификация](benchmarks/cross-copy-captured-core-qualification-2026-11-02.json).
Это не квалификация разных версий, разных realms или автоматического контекста
через пользовательский await.

### 1. Закрыть ближайшие lifecycle и cross-copy сценарии

- [x] Проверить в Edge реальное DOM-событие сохранённого обработчика успешного
  child после закрытия carrier: callback копии B меняет signal и создаёт computed
  копии A; до завершения parent работает, после teardown не вызывается.
  Незахваченное позднее продолжение должно по-прежнему получать отказ.
  Приёмка: воспроизводимый отказ на предыдущем runtime, успех на исправленном,
  стабильный и beta workerd profiles, GC с retained DOM и installed-package gate.
  Все проверки прошли; 24/24 целевых объекта собраны.
  [Квалификация](benchmarks/edge-retained-child-qualification-2026-11-02.json).
- [x] Проверить встроенные Suspense/ErrorBoundary между копиями: видимость
  pending/error от foreign resource, восстановление и teardown. Сначала fixture:
  совместное использование пользовательского Context не доказывает этот контракт.
  Исправлены встроенные protocol Symbols; оба направления, pending/completion/
  cancellation и rejection/retry прошли в JSDOM и installed gate (4/4).
  Edge workerd/LinkeDOM: 4/4 на stable и beta/November, installed gate прошёл.
  Дополнительно исправлен retry batch в чужом ambient core: content больше
  не создаётся дважды. Полный suite 1294/98. Chrome156 native DOM: 4/4
  на source production bundles и 4/4 на installed production bundles.
  [Browser квалификация](benchmarks/browser-copy-boundaries-qualification-2026-11-02.json).
  Другие браузеры и настоящий browser async-context carrier ещё открыты.
  [Edge квалификация](benchmarks/edge-copy-boundaries-qualification-2026-11-02.json).
  [Квалификация](benchmarks/cross-copy-boundaries-qualification-2026-11-02.json).
- [ ] Реализовать и квалифицировать автоматические зависимости между разными
  native cores либо явно закрепить ограничение публичного контракта. Сейчас
  cross-copy tracking probe проходит 11/12 сценариев; отдельный failing case
  сохраняется открытым. Для реализации нужны batching, dynamic dependencies,
  disposal/reset и циклы; общей JS-подписки недостаточно.

- [x] Исправить built-in RouterContext между копиями и атомарность navigate
  из другого ambient core. Installed probe 3/3: useRouter/Link в обоих
  направлениях, route cleanup, teardown и согласованная публикация URL.
  JS1295/99, TypeScript, package с Edge GC и HTTP actions4/4 прошли.
  Chrome156 installed production bundles: 3/3 native DOM/Link/URL checks
  прошли. Native back/forward, denied-Back compensation и redirect с foreign
  observers также прошли в обоих направлениях (общая matrix5/5).
  [History квалификация](benchmarks/browser-copy-history-qualification-2026-11-02.json).
  Rapid bursts: isolated A→B и B→A по11/11 прошли (по6 серий), но общий
  прогон16/17 завершился setup timeout последнего сценария. Причина открыта;
  Timeline повторил16/17: после финального go(-2) нет popstate за3с.
  Native-only replay199 команд завершился успешно; простой лимит числа/темпа
  команд этим контролем не подтверждён. Следующий шаг — сравнить interleaving
  popstate/команд и синхронные компенсации на границах сценариев.
  [Диагностика](benchmarks/browser-copy-burst-timeline-analysis-2026-11-02.json).
  Дополненный native replay с двумя обычными Link-навигациями воспроизводит
  отсутствие финального popstate без Svet (201 команд). Fixture больше
  не запускает эти побочные навигации. Полная matrix17/17 проходит с явной
  паузой между сценариями после плотной серии history-команд; непрерывный
  high-rate прогон остаётся открытым.
  [Контроли](benchmarks/browser-copy-burst-controls-2026-11-02.json).
  [Partial evidence](benchmarks/browser-copy-burst-qualification-2026-11-02.json).
  Edge router, общий burst-прогон и конкурирующие routers ещё открыты.
  [Browser квалификация](benchmarks/browser-copy-router-qualification-2026-11-02.json).
  [Квалификация](benchmarks/cross-copy-router-qualification-2026-11-02.json).

- [x] Убрать выявленную стоимость пустого shared-core stack: guard длины
  перед чтением top. Только wasm-bridge.js отличается в A/B; synthetic
  untrack median ratio0.29–0.43, с шумными AA-контролями. JS1295/99, types,
  package с Edge GC и actions4/4 прошли. Это не performance gate приложений;
  повтор без конкурирующей нагрузки и multi-copy замеры остаются открытыми.
  [Квалификация](benchmarks/core-stack-fastpath-qualification-2026-11-02.json).

- [x] Освободить ErrorBoundary closure при сохранённом retry после teardown.
  GC before4/4 cores retained → after0/4, controls4/0; package gate требует
  этот сценарий. JS1295/99, types, Edge/package и actions4/4 прошли.
- [x] Исправить удержание disposed Router: before4/4 cores/memory → after0/4
  при сохранённых router objects и guard-removers. Terminal readers/methods
  сохраняют snapshots, поздний navigate=false; package gate держит12 ссылок
  и собирает8 cores+8 Memory. JS1296/99, types, package/Edge GC, actions4/4
  прошли. Предполагаемый1 node оказался inactive effect pool; public telemetry
  подтверждаетgraph0 до и после исправления. Chrome156 installed runtime:
  history17/17 (paced), natural GC под bounded pressure собирает16 cores+
  16 Memory при40 сохранённых ссылках, controls4→0. Edge/другие браузеры
  и длительные bounds ещё открыты.
  [Browser квалификация](benchmarks/retired-ui-browser-qualification-2026-11-02.json).
  Local Edge stable и beta/November:16 requests,40 сохранённых ссылок,
  before32/32 → after0/32 core/Memory targets; controls4→0, inspector ACK.
  Installed-package gate требует этот сценарий. Natural cadence, deployed
  providers и Edge History API этим не квалифицированы.
  [Edge квалификация](benchmarks/edge-retained-ui-qualification-2026-11-02.json).
  [Квалификация](benchmarks/retained-router-qualification-2026-11-02.json).
  [GC evidence](benchmarks/retained-ui-callbacks-qualification-2026-11-02.json).

- [x] Восстановить замену options.routes у живого Router после выделения
  чистого matcher. После dispose используется последняя прочитанная таблица,
  options больше не читается/не удерживается. Installed4/4, JS1297/99
  (one worker, лимит benchmark5s сохранён), types/actions/package+Edge GC
  прошли. Chrome156 installed bundles: before17 pass/2 fail → after19/19
  (paced), включая foreign RouterView и terminal matcher в обоих направлениях.
  Длительный soak этой редакции ещё открыт.
  [Browser квалификация](benchmarks/router-table-browser-qualification-2026-11-02.json).
  [Квалификация](benchmarks/router-route-table-qualification-2026-11-02.json).

- [x] Проверить Rust → embedded → installed WASM локально:53 native tests,
  две независимые release/simd128 сборки одинаковы,54320 bytes; source embed,
  exports и установленный asset/runtime совпадают. Валидный byte-different
  control отвергается. CI guard добавлен до перегенерации embed и в installed
  gate через SVET_REBUILT_WASM; local package+Edge gate прошёл. Remote Linux
  CI/cross-host воспроизводимость ещё не проверены; общий P4 остаётся открытым.
  [Квалификация](benchmarks/rust-wasm-provenance-qualification-2026-11-02.json).

- [x] Исправить AOT .ui module registry: production больше не удерживает
  созданные/не mounted instances; HMR регистрирует только успешные mounts,
  сохраняет entry до последнего unmount и удаляет при dispose. Before3 fail/
  8 targets retained → after3/3,0/8; controls4→0. JS1297/99, types, actions4/4,
  package с Rust provenance и Edge GC прошли. Реальный Vite HMR/browser ещё открыт.
  [Квалификация](benchmarks/forma-aot-lifecycle-qualification-2026-11-02.json).

- [x] Убрать общий unsynchronized native C-ABI graph: native calls теперь
  используют thread-local RefCell; owned ReactiveGraph предназначен для
  отдельных окон на одном потоке. Before child видел parent node; after55 Rust
  tests, включая8×256 concurrent flush cycles, прошли. Новый WASM re-embedded,
  две сборки одинаковы; JS1297/99, types/actions/package+Rust provenance+Edge GC
  прошли. Реальный native Studio adapter остаётся открытым.
  [Квалификация](benchmarks/native-abi-thread-qualification-2026-11-02.json).

- [x] Добавить native owned graph context для будущего window adapter:
  identity/lifetime-safe handles, stale jobs/reset/reuse refusal, dynamic
  tracking, nested computed, cycle guard и batch/unwind scopes. Rust66/66,
  native binding projection example и wasm32 example compile прошли.
  WASM/runtime20 файлов byte-equal прежней1297/99-qualified редакции.
  CI запускает native example; подключение к Forma native model/renderer,
  callbacks/values и component ownership остаётся открытым.
  [Квалификация](benchmarks/native-owned-context-qualification-2026-11-02.json).

### 2. Завершить длительную проверку памяти и транспорта

- [x] Получить final report frozen двухчасового natural-GC HTTP
  soak, проверить неизменность runtime/harness и анализировать все четыре окна.
  Progress-файл не считать результатом. Бюджеты роста: heap 16 MiB, RSS 64 MiB,
  external 64 MiB, ArrayBuffer 16 MiB; сохранить raw и вывод анализатора.
  [Метаданные запуска](benchmarks/http-natural-frozen-run-2026-11-02.json).
  Final: 1 232 608 requests /7200032ms, без request/cleanup failures,
  forcedGC0, graph0, hashes unchanged. Все4 memory budgets прошли по
  post-warmup window envelopes. Это ранняя baseline, не текущий runtime.
  [Квалификация](benchmarks/http-natural-frozen-2h-qualification-2026-11-02.json).
- [ ] Установить исходную причину отказа предыдущего прогона после ~24 минут;
  исправленная primary/cleanup диагностика сама по себе причину не устраняет.
- [ ] Повторить длительную квалификацию на окончательном новом runtime:
  текущий frozen запуск относится к более раннему installed artifact и не
  проверяет новые shared-domain/owner/core-scope изменения.
  Frozen LPJBKt завершён и проанализирован:918440 requests, все4 бюджета прошли.
  Актуальная редакция отличается4 runtime files; отдельный latest2h прогон09qhPH запущен.
  [Метаданные](benchmarks/http-natural-current-run-2026-11-02.json).
- [ ] Расширить нагрузку: оба codecs, gzip, медленные клиенты, крупные patches,
  disconnect/abort, насыщение сокетов; проверить bounded queues и восстановление.
  Затем проверить выбранный production proxy/CDN, TLS и HTTP2/3.

### 3. Расширить матрицу сред и renderer

- [ ] Другие браузеры и реальные async-context carriers: nested/concurrent await,
  rejection, abort, streaming, capture и teardown. Проверить native forms APIs.
- [ ] Развёрнутые Edge providers, natural GC, stable Miniflare после beta;
  отдельно cancel/FIN propagation через proxy и mixed-version deployments.
- [ ] Физические GPU/backends: queue cancellation, повторные device loss/recovery,
  resize/DPR, resident buffers и renderer lifetime. Сопоставить projection,
  picking, clipping/depth и перекрывающуюся/невыпуклую геометрию с эталонами.

### 4. Проверить пользовательские приложения и подготовить доставку

- [ ] Реализовать native Svet adapter: текущий --svet Studio ведёт в общий
  vector paint_native; owned core теперь подключён к typed Form и retained
  Document scheduling. Ещё нужны полный
  graph/renderer/lifetime bridge и доказанный end-to-end на native backend.
  [Аудит](benchmarks/native-studio-svet-integration-audit-2026-11-02.json).
- [ ] Пройти production end-to-end Forma/compiler и native studio: запуск,
  взаимодействие, обновление и teardown.
- [ ] Выполнить A/A и A/B на реальных приложениях: startup, p95, frame time,
  long tasks, transfer bytes, очередь/readback и память; повторить на другом GPU.
- [ ] Свести supported/partial/unsupported по функциям и средам с актуальными
  артефактами; закрепить CI, чистую установку, exports/types и соответствие
  Rust → embedded WASM → installed archive. Обновить документацию.
- [ ] Commit/push/release — отдельный запрошенный этап после проверки состава
  изменений и сохранения посторонней работы.

Для каждого пункта: fixture и критерий приёмки → исправление и регрессия →
проверка установленного пакета → raw evidence и ограничения. Новые дефекты
корректности и утечки имеют приоритет перед измерениями производительности.

## Подробная очередь и история на 2026-11-01

Эта историческая очередь дополняет актуальный рабочий план выше. Здесь перечислена
оставшаяся работа, а реализованные части указаны как исходная база. Узкая проверка
не закрывает весь пункт. Проверки привязаны к конкретным логам и артефактам.

Базовая завершённая квалификация request/render tokens: **1286 тестов / 97 файлов**, TypeScript,
installed-package gate с workerd (8 concurrent events без keepalive) и HTTP
actions4/4 прошли. [Qualification](benchmarks/edge-gc-qualification-2026-11-01.json)
связывает source/runtime hashes, snapshots, installed и GC evidence.
[JS лог](benchmarks/edge-gc-final-suite-2026-11-01.txt),
[package лог](benchmarks/edge-gc-qualified-package-2026-11-01.txt),
[actions лог](benchmarks/edge-gc-final-actions-2026-11-01.txt).
Retained-error GC: 32 конкурентные операции, 96 объектов, 0 удержанных при
сохранённых errors/handles ([отчёт](benchmarks/ssr-timeout-retention-final-2026-11-01.json)).
Это короткая Node-проверка, не доказательство длительного плато или browser/Edge GC.

### P0 — завершить текущую правку и проверки времени жизни

- [x] Исправить зависание timeout Web reader.closed при8 workerd fetch events.
  Adapter удерживает native stream до завершения producer teardown и затем
  снимает ссылку. Контролируемый откат возвращает отказ на той же fixture.
  Без keepalive прошли60с:1464 events,23424 DOM cycles,11712 streams и столько
  же string renders,1464 timeouts. Direct HTTP5с:416 events. Installed gate
  требует обычный readable transport/8 concurrent requests и отвергает diagnostics.
  GC432 objects collected, включая16 discarded unstarted streams.
  [Qualification](benchmarks/edge-stream-lease-qualification-2026-11-01.json).
  Точный native heap-retainer path не снят; Edge GC и многочасовые границы
  не следуют из этого результата и остаются открытыми.

- [x] Добавить регрессию: пользовательский `Error.prepareStackTrace` бросает
  исключение при фактическом timeout; запрос всё равно завершается типизированным
  `SsrTimeoutError`, таймер и request graph освобождаются.
- [x] После последней правки последовательно выполнить build:package, TypeScript,
  полный JS suite, installed-package gate с optional Edge, HTTP actions и
  `git diff --check`; сохранить логи и привязать их к исходникам.
- [x] Провести конкурентный deadline churn: success/timeout/caller abort/child
  timeout, отмена части запросов при живых соседях, сохранённые errors/handles.
  Проверять исходные abort reasons, cleanup ровно один раз, отсутствие поздних
  записей, остаточных timers/listeners и удержанных cores/WASM memory.
  Выполнена Node Web-stream часть: 60с, 16 live requests, оба codecs,
  8336 операций (по 2084 complete/cancel/abort/timeout), 50016 объектов собраны
  при сохранённых terminal errors/handles; cleanup1, graph0 и caller listeners0.
  [Raw](benchmarks/ssr-deadline-churn-60s-2026-11-01.json).
  Child scopes: 60с, 21832 parents / 87328 children, 43664 late refusals,
  152824 объектов собраны при сохранённых errors; активных таймеров0 после
  каждой группы. [Raw](benchmarks/child-deadline-churn-60s-2026-11-01.json).
  Web-stream timer accounting отдельно прошёл160 операций:
  [raw](benchmarks/ssr-deadline-timers-2026-11-01.json).
  Оба коротких варианта включены в package gate. Длительные HTTP/Edge нагрузки
  и многочасовое плато остаются в P1.
- [ ] Расширить GC-проверки на браузерные receiver/DOM и Edge lifecycle.
  Сделано: локальный workerd /24 requests /6 paths /2 codecs,140 целевых
  объектов собраны при сохранённых terminal pairs/errors; controls4 до release,
  0 после. Inspector GC команды подтверждены. Retained host request/render tokens
  больше не держат core/publisher; зависший view также закрывает render session.
  [Qualification](benchmarks/edge-gc-qualification-2026-11-01.json).
  Owner-channel tokens также исправлены: установленный Node-пакет собирает
  17 cores при удержанных 17 request / 13 owner / 11 render tokens;
  [package log](benchmarks/owner-token-package-2026-11-01.txt).
  Полный JS suite: 1286 тестов / 97 файлов;
  [лог](benchmarks/owner-token-qualified-suite-2026-11-01.txt).
  Осталось: другие providers/браузеры, естественная GC cadence и длительные
  RSS bounds. Этот общий пункт пока открыт.
  Chrome156 browser GC: 32 terminal receiver containers и 16 select signals
  собраны при сохранённых handles/detached select DOM. Live controls4 до release,
  0 после; graph0. Два прогона, natural GC под bounded allocation pressure,
  не CDP forced GC. [Raw](benchmarks/browser-retention-gc-2026-11-01.json).
  Открыты SSR request-core/browser carrier, Edge GC и другие browsers.
  Node probes iterator/receiver/readable уже есть; они не доказывают browser GC.
  Edge DOM lifecycle квалифицирован в локальном workerd+LinkeDOM: два
  concurrent events / 32 cycles, keyed identity/index updates, removed scopes,
  Show, SVG, escaped text и event teardown. Graph0/cleanup подтверждены;
  это не GC-доказательство и не deployed-provider matrix.
  [Raw](benchmarks/edge-dom-lifecycle-qualified-2026-11-01.json).
  Forms выявили getter-only select.value в LinkeDOM; model исправлен через
  option.selected. Text/checkbox/textarea/single-select SSR прошли в workerd;
  [raw](benchmarks/edge-forms-after-2026-11-01.json). Multiple select, reset,
  form events и остальные DOM APIs требуют отдельной проверки.
  Исправлен select value без model: initial static/reactive значение применяется
  после options, getter-only host поддержан. Suite1281/96, package с Edge,
  actions4/4 прошли; [raw](benchmarks/select-value-edge-2026-11-01.json).
  Поздние options исправлены: mutation checkpoint обновляет selection при
  вставке/удалении/перестановке и text/value changes; cached value не вызывает
  accessor повторно. Unmount/reset защищены. Suite1285/96 и package с Edge
  прошли; [raw](benchmarks/select-options-edge-2026-11-01.json).
  Multiple-select host path исправлен и квалифицирован: missing multiple/
  selectedOptions reflection, initial/reactive выбор, change roundtrip и HTML
  с несколькими selected options. Suite1286/97, package с Edge прошли.
  [Raw](benchmarks/edge-multiple-after-2026-11-01.json).
  Native form reset/validation в Edge и естественная GC cadence остаются открытыми.

Критерий закрытия: регрессии входят в package gate, полные проверки проходят
на последней редакции, raw результаты и ограничения записаны в DEVELOPMENT.md.

### Ближайшая последовательность работ

1. [x] Завершить owner-token этап: финальные hashes и raw Node/Edge отчёты
   сведены в [qualification receipt](benchmarks/owner-token-qualification-2026-11-01.json),
   HTTP actions4/4 и TypeScript прошли; DEVELOPMENT/матрица обновлены.
2. [x] Закрыть transport error matrix бинарника workerd2026-11-01: producer
   failure до Response, EOF refusal, cleanup и fresh-request recovery проверены.
   [Raw](benchmarks/edge-transport-new-qualified-2026-11-01.json).
   Controller/compatibility date закреплены на July; новая Miniflare/compat date открыты.
3. Двухчасовой Node HTTP natural-GC soak завершился отказом после189144
   запросов/~24мин: timeout cleanup-запросаforget. Хэши не изменились;
   причина исходного отказа пока не установлена. Сохранённый final raw:
   benchmarks/http-natural-2h-2026-11-01.json. Primary/cleanup диагностика исправлена и проверена controlled503/503,
   natural/forced pilots. Повторить длительную квалификацию после установленного
   package gate новой редакции прошёл; исходная причина24-минутного отказа открыта.
   Повтор запущен на frozen harness + installed package, все20 JS/WASM files
   зафиксированы: http-natural-frozen-2h-2026-11-02.json.progress.json.
   [Harness receipt](benchmarks/http-frozen-harness-qualification-2026-11-02.json). Полный HTTP/Edge soak с обоими
   codecs/gzip, насыщением сокетов и длительными memory bounds остаётся открыт.
4. Native browser constraint-validation fixture прошла13/13 на текущих
   исходниках; reset regressions30/30. Installed production matrix21/21,
   189 assertions также прошла. Квалифицировать другие browsers и Edge native APIs; расширить browser/Edge lifecycle matrix; затем выполнить GPU/renderer и Forma/native studio проверки из P2–P3.
5. Собрать итоговую матрицу готовности и обязательные CI gates из P4.

Для каждого шага: сначала воспроизводимый сценарий и критерий приёмки, затем
реализация/регрессия, проверка установленного пакета и raw evidence. Найденный
дефект корректности или утечка получает приоритет перед оптимизациями.
Commit/push/release выполняются отдельным запрошенным шагом.

### P1 — длительная нагрузка и transport/SSR совместимость

- [x] Квалифицировать workerd2026-11-01 binary override на закреплённом July
  controller/compat date: fetch rejection до Response и truncated EOF различаются,
  producer failure подтверждён, cleanup1/graph0 и fresh-request recovery проходят
  для обоих codecs. [Raw](benchmarks/edge-transport-new-qualified-2026-11-01.json).
- [x] Проверить доступную Miniflare5 beta с matching October workerd/date и
  November binary/date override; официальный v4-options converter подключён.
  [Raw](benchmarks/edge-beta-november-2026-11-01.json). Stable default сохранён.
- [ ] Повторить beta matrix после stable-релиза; отдельно квалифицировать remote
  deployment, многочасовые границы и natural GC cadence.

- [ ] Провести длительный конкурентный HTTP soak с медленными клиентами,
  крупными patches, disconnect, gzip и обоими codecs. Наблюдать реальную
  backpressure/насыщение сокета, bounded queues и восстановление после abort.
  Добавлена concurrent16-case matrix: оба codecs, gzip on/off, complete, shell
  disconnect, pending-patch disconnect/host abort; 94 rounds/1504 requests прошли.
  [Raw](benchmarks/transport-matrix-qualified-2026-11-01.json).
  Существующие 60-секундные loopback-прогоны не доказывают многочасовое плато.
- [ ] Измерять отдельно JS heap, RSS, external/ArrayBuffer, WASM и GPU память;
  зафиксировать прогрев, нагрузку, интервалы и допустимый рост. Для роста снять
  heap/retainer evidence; оценить временные копии и стоимость ограничения арены.
- [ ] Проверить production proxy/CDN и TLS/HTTP2/3 на конкретном deployment target.
  Отдельно разобрать непередачу cancel/FIN через закреплённый Miniflare proxy:
  host deadline ограничивает lifetime, но propagation пока не исправлен.
- [ ] Квалифицировать mixed server/client deployments, несовместимые версии,
  capabilities, CORS и malformed frames через настоящий HTTP и браузер.
  Negotiation helpers и wire v1 уже реализованы; ошибка должна возникать до DOM mutation.
- [ ] Проверить progressive HTTP latency, peak memory и CPU для identity/ref codec
  с gzip и без него. Сохранить DOM/input identity и cross-boundary coherence;
  codec принимать по workload, а не по одному уменьшению bytes.
- [ ] Расширить Edge DOM/churn matrix, проверить развёрнутый host, определить
  поддерживаемые DOM API. Затем квалифицировать другие Edge providers и настоящий
  browser async-context carrier: nested/concurrent await, rejection, abort,
  streaming, teardown. Неподдерживаемые среды явно отклонять/документировать.

Критерий закрытия: воспроизводимые отчёты на названных средах, bounded resources,
изоляция запросов, подтверждённые terminal paths; непроверенные среды не объявлены поддержанными.

### P2 — браузеры, GPU, async-контракт и renderer correctness

- [ ] Расширить [BROWSER_MATRIX.md](BROWSER_MATRIX.md): дополнительные браузеры,
  физические GPU/backends, resize/DPR, queue cancellation, повторные device loss/recovery.
  Разделять software и hardware результаты.
- [ ] Проверить capture после await и
  tracking между несколькими копиями движка; закрепить контракт тестами.
  Cross-copy untrack исправлен на исходниках и проверен двумя независимыми
  сборками: suppression/new observers/throw/reset. Installed package gate с Edge/GC прошёл; длительный soak завершился отказом; foreign signal dependency остаётся
  failing/open. [Installed partial receipt](benchmarks/cross-copy-weak-installed-qualification-2026-11-02.json).
  Протоколv2: локальный hot path + слабый discovery; performance gate пока открыт.
  GPU fallback thenables проверены; sync double-getter defect исправлен,
  JS1293/98 + TypeScript/package с Edge прошли.
  [GPU receipt](benchmarks/gpu-thenable-qualification-2026-11-01.json).
  Resource PromiseLike контракт и hostile-thenable lifecycle проверены:4 новых
  регрессии, JS1290/98, TypeScript и package с Edge прошли.
  [Receipt](benchmarks/resource-thenable-qualification-2026-11-01.json).
- [ ] Сопоставить 2D/GPU projection, picking, clipping/depth на невыпуклых,
  перекрывающихся объектах и граничных координатах; сохранить визуальные эталоны.

Критерий закрытия: явная матрица supported/partial/unsupported с артефактами,
регрессиями и проверенным освобождением GPU/DOM ресурсов.

### P3 — производительность приложений и интеграция Forma

- [ ] Выполнить воспроизводимые A/B и A/A на пользовательских приложениях:
  startup, p95/frame time, long tasks, bytes и память. Повторить в отдельном
  запуске и на другом оборудовании; отдельно измерить shallow-untrack regression
  и цену lifecycle guards.
- [ ] Проверить compiler/Forma и native studio end-to-end на актуальных production
  артефактах; отдельно обычный compiler mode, запуск, взаимодействие и teardown.

Критерий закрытия: наблюдаемое поведение совпадает с эталоном, выигрыш выдерживает
контроли и повторные запуски; отсутствие регрессий подтверждено, а не выведено из FPS.

### P4 — готовность и доставка

- [ ] Собрать итоговую матрицу функций и сред с implemented/validated/partial/
  unsupported, ссылками на raw evidence и оставшимися ограничениями.
- [ ] Закрепить обязательные CI gates, чистую установку, exports/types,
  Rust → embedded WASM → installed archive и воспроизводимость артефактов.
- [ ] Актуализировать README/DEVELOPMENT и инструкции запуска/диагностики.
- [ ] По отдельному запросу подготовить commit/push/release; перед доставкой
  проверить branch/remote, состав изменений и доставленный артефакт.

Критерий закрытия: проверенная чистая установка и согласованная матрица готовности.
Публикация сама по себе не закрывает незавершённые P0–P3.

### Уже сделано — база для оставшейся работы

Ниже сохранены результаты отдельных этапов. Старые числа и формулировки
«ещё открыто» читаются вместе с более поздними результатами и очередью выше.

Stream receiver retention исправлен: публичный handle хранит только очищаемую
ссылку на внутренний receiver. Teardown разрывает её до очистки resumed bindings.
GC probe с сохранёнными handles прошёл для dispose/reset/replacement/protocol error:
0/32 удержанных detached containers; контроль также 0/32. Исходный отказ 32/32
сохранён. Raw: [stream-receiver-retention-after](benchmarks/stream-receiver-retention-after-2026-11-01.json).
Probe добавлен в installed-package gate. Полный JS suite: 1243/88 passed.
Iterator, complete без dispose, request-core retention и browser GC ещё открыты.

Terminal stream iterator retention исправлен: сохранённые handles после
полного чтения/return/error не удерживают view/captured objects/core/memory/node.
До исправления: 24 view/captures и 8 core/memory; после — 0 всех типов.
Raw: [stream-iterator-retention-after](benchmarks/stream-iterator-retention-after-2026-11-01.json).
Полный suite 1244/88 прошёл; concurrent next regression включена.

External abort без terminal чтения квалифицирован: отмена до запуска и после
shell освобождает view, поздний next один раз возвращает исходную ошибку.
GC probe40 handles: 0 удержанных объектов, JS1245/88 passed.
Raw: [stream-abort-retention-after](benchmarks/stream-abort-retention-after-2026-11-01.json).
Незапущенный неотменённый iterator сохраняет view для будущего вызова.

Concurrent HTTP streaming soak60с прошёл: 8 clients, 11952 requests,
7968 complete /3984 disconnect,15936 writer drains; после каждого запроса
nodes0/cleanup1. Heap/RSS/external/arrayBuffers записаны, sample ring32.
Raw: [stream-http-soak](benchmarks/stream-http-soak-60s-2026-11-01.json).
Это delayed fetch reads на loopback, не подтверждение TCP saturation/proxy
buffering или многочасового плато.

Local HTTP proxy matrix4/4 прошла: throttled relay и full-buffering proxy,
complete/disconnect в обоих режимах. Streaming даёт ранний shell, buffering
его скрывает; оба disconnect закрывают origin с nodes0/cleanup1.
Raw: [stream-proxy](benchmarks/stream-proxy-qualified-2026-11-01.json).
Production proxy/CDN, TLS/HTTP2/3 и длительное socket saturation открыты.

Opt-in HTTP version negotiation реализован публичными helpers:
server выбирает v1 до SSR, client проверяет headers до body/receiver.
Native HTTP/JSDOM7 checks прошли; JS1247/89. Raw:
[stream-http-negotiation](benchmarks/stream-http-negotiation-2026-11-01.json).
Включение policy в каждый production route остаётся задачей host; mixed real
versions в браузере, CORS и capabilities ещё открыты.

Перед оптимизацией исправлена topology validation boundaries: crossing ranges
и noncanonical IDs отклоняются; shell/patch fragments проверяются до замены
старого содержимого. Регрессия воспроизведена до fix; JS1250/89 passed.
Installed gate содержит crossing-shell regression. Final HTML duplication
этой правкой не уменьшено.

Optional HTML-reference transport codec реализован: decoded wirev1 не меняется,
повтор предыдущего HTML заменяется reference+prefix/suffix при меньшем JSON.
Fixture plain520757→encoded260752 bytes, complete260300→295; records exact,
input identity/resume сохранены; JS1255/90 passed. Raw:
[stream-codec](benchmarks/stream-codec-2026-11-01.json).
Это opt-in transport, не auto-negotiated формат. Final serialization CPU,
реальный HTTP/browser codec и paired latency/memory ещё открыты.

HTTP codec negotiation и installed browser integration квалифицированы:
explicit request/response codec header, identity default, unknown offer406 до SSR,
identity client отвергает encoded ответ до body. Installed HTTP4 checks и
production Chrome7/7 прошли, bytes248/132403/301; runtime18 JS hash match.
JS1256/90 и package9/9 passed. Evidence:
[stream-codec-browser](benchmarks/stream-codec-browser-qualification-2026-11-01.json).
CORS, malformed-frame browser paths и mixed deployments остаются открытыми.

Isolated paired transport benchmark выполнен:72 processes/864 measured
roundtrips,3 workloads,interleaved plain/codec/gzip/codec-gzip и A/A.
Varied codec+buffered-gzip processing−26…−31% к gzip (A/A<1%), repeated+19…25%
при102 bytes экономии. Без gzip codec добавляет CPU; universal win не заявляется.
Лишняя повторная сериализация chunk удалена; JS1256/90 passed.
Raw: [codec-paired](benchmarks/stream-codec-paired-qualified-2026-11-01.json).
Память только sampled, gzip buffered: progressive HTTP latency/peak/plateau
остаются открытыми.

Progressive gzip fixture квалифицирован через native HTTP и production browser:
identity-gzip4 checks/codec-gzip5 checks, package9/9, Chrome7/7. Shell приходит
до release,3 sync flushes,1 compressor drain,715 compressed body bytes, graph0.
Evidence: [progressive-gzip](benchmarks/stream-progressive-gzip-qualification-2026-11-01.json).
Это explicit test-host compression, не Accept-Encoding policy для любого host.

Active large gzip write cancellation проверен:4MiB varied patch, identity/ref
×disconnect/host abort. Во всех4 cases наблюдались pending drain, живой graph,
compressed output>4096; после abort pipeline settled/compressor closed/nodes0/
cleanup1. Fresh compressed stream восстановился. Raw:
[active-gzip-abort](benchmarks/stream-gzip-active-abort-qualified-2026-11-01.json).
Pending flush отдельно не подтверждён, socket saturation не заявляется.

Pending native gzip flush квалифицирован deterministic host-abort injection:
12 pending flush aborts +12 successful recoveries,6 cycles на каждый codec.
Во всех случаях graph0/cleanup1/pipeline settled/compressor closed; package9/9.
Raw: [pending-flush](benchmarks/stream-gzip-pending-flush-qualified-2026-11-01.json).
Это не remote timing race, socket saturation или long memory plateau.

Node Web ReadableStream adapter реализован: lazy pull/HWM0, UTF8 NDJSON,
identity/ref codec, cancel/pending next, external abort/owner disposal. JS1263/91,
TypeScript/build passed; native Response→Readable.fromWeb→HTTP4 checks passed.
Raw: [readable-SSR](benchmarks/readable-ssr-http-qualified-2026-11-01.json).
Это Node ALS entry, не portable Edge ownership implementation.

Readable terminal notification исправлена по2 failing regressions: background
producer error сообщает reader.closed без read; complete сам закрывает stream.
JS1265/91, TypeScript/build passed. Retained stream+reader GC64 requests/384
objects:0 alive, обе codecs и4 terminal modes. Raw:
[readable-retention](benchmarks/readable-ssr-retention-qualified-2026-11-01.json).
Long RSS/throughput/churn plateau этим не доказано.

Concurrent Readable soak60с квалифицирован:16 live cores/barrier,64720 requests,
по16180 complete/cancel/abort/error. Отмена половины не портит остальные cores;
каждый final response проверен по собственному ID. Все388320 WeakRef objects
collected при сохранённых terminal pairs; heap/RSS последних окон ниже ранних.
Raw: [readable-concurrent](benchmarks/readable-concurrent-soak-60s-2026-11-01.json).
Installed short probe включён в package gate,9/9 passed. Это не HTTP/browser/
Edge throughput, не многочасовой memory bound.

Host-neutral SSR runtime выделен: server.ts — Node ALS/UUID adapter, общий
runtime доступен через ./server-runtime. Carrier contract getStore/run/exit,
independent request/owner/render channels, bounded native-await/concurrency/
exit/error probe; sync stack rejected. JS1268/92, TypeScript/build и package9/9
passed; installed browser build напрямую импортирует runtime без Node builtins.
Injected Node host проверен отдельно; actual browser/Edge provider не готов.

Initial real Edge host qualified: local workerd native ALS +LinkeDOM,
2 concurrent fetch events/16 async renders, own values/isolated cores/cleanup.
Dynamic WASM compile rejection reproduced and fixed with public precompiled
asset +installSvetWasmModule before bootstrap. Export shape/lifetime guards
and same-build asset packaging verified. Rust53/build/JS1269/93 passed.
CARGO_TARGET_DIR embedding bug fixed;3 target-path regressions passed.
Raw: [workerd-installed](benchmarks/edge-ssr-installed-qualified-2026-11-01.json).
This is local workerd string SSR, not deployed Cloudflare/general Edge support.

Workerd streaming ownership qualified inside fetch events:16 concurrent cases,
4 terminal modes ×2 codecs; own values/cores/snapshots,8 late refusals. Direct
Response6-case matrix passes complete/error/TCP reset; Request.signal observed
before component completion with cleanup1/nodes0. Added receiver.finish() to
reject truncated EOF;3 regressions, JS1272/93 and optional Edge package9/9 passed.
Raw: [edge-stream](benchmarks/edge-stream-qualification-2026-11-01.json).
Miniflare proxy reader cancellation/normal FIN probes still fail to propagate
abort and are explicitly unqualified; failure evidence retained.

Explicit proxy request AbortController also fails to reach incoming Worker
signal in the pinned Miniflare path; negative evidence preserved. Optional
host deadline500ms qualified for both codecs: pending graph retired before
5000ms component completion, cleanup1/nodes0, deadline true/incoming abort false.
Raw: [proxy-deadline](benchmarks/edge-proxy-deadline-qualified-2026-11-01.json).
This bounds lifetime without claiming proxy cancellation fixed.

Shared runtime timeoutMs implemented for isolated/child/string/chunks/readable:
SsrTimeoutError, caller reason preservation, timer/listener release and child
isolation. JS1276/94, TypeScript/build and optional Edge package9/9 passed.
Real workerd Web stream timeout verified in2 fetch events: cleanup1/graph0.
Raw: [runtime-timeout](benchmarks/edge-runtime-timeout-2026-11-01.json).
Default remains no timeout; host timer cannot preempt synchronous CPU work.

Retained timeout error GC: исходный probe воспроизвёл удержание 8 child cores
и 8 WASM memories; перенос создания ошибки до таймера сам по себе не помог.
Преобразование stack в строку устранило удержание в Node probe. Последний вариант
создаёт ошибку только при истечении срока; 32 concurrent operations / 96 objects
collected при сохранённых errors/handles. Точный heap-retainer path отдельно не снят.
Raw: [expiry GC](benchmarks/ssr-timeout-retention-expiry-2026-11-01.json).
Полный suite после этой редакции прошёл: 1278/94; package с Edge и actions также прошли.

Следующий конкретный этап: owner-channel token GC, затем длительные HTTP/RSS bounds. Остальные задачи выполняются
по очереди P1–P4 выше; исходные отказы и результаты сохраняются.

## История этапов и подробные ограничения

Пункты ниже фиксируют ход работ. Их старые формулировки и числа тестов
не заменяют актуальную очередь выше.

### P0. Завершить текущий замер реального GPU-приложения

- [x] Проверить и запустить production fixture `tests/browser/resident-app-bench.html`
  по протоколу `interleaved-controls-v2`: предварительный прогрев, калибровка,
  чередование mapped/resident и A/A-контроли между раундами.
- [x] Сохранить новый raw JSON отдельно от пилота, параметры, версии, аппаратный
  адаптер, хеши исходников и production bundle. Независимо пересчитать p50/p95,
  проверить число кадров, порядок раундов, ошибки и освобождение ресурсов.
- [x] Подтвердить совпадение геометрии и пикселей; сравнить bytes upload/readback,
  latency, RAF intervals, long tasks и живые GPU-буферы. Отдельно указать
  постоянную память и пиковые allocations. RAF cadence не считать доказанным FPS.
- [x] Записать вывод и границы измерения в DEVELOPMENT.md. Пилот сохранён в
  `benchmarks/resident-app-pilot-metal-2026-11-01.json`: обнаружен временной дрейф,
  поэтому он пока не доказывает устойчивое ускорение. Наличие кода v2 ещё
  не означает, что повторный аппаратный прогон выполнен.
- [x] Проверить preparer, production fixture и package checks после последних
  изменений; сохранить итоговый отчёт и `git diff --check`.

Готово, когда результат воспроизводим, A/A показывает величину шума, а вывод
учитывает дрейф и компромисс памяти. Если шум перекрывает эффект, зафиксировать
неопределённость и причину следующего эксперимента.

Результат v2: 3240 измеренных кадров, 10 A/B и 3 A/A пары на Apple Metal 3.
На большой сетке p50 −39…−45%, A/A p50 −9…+4%; малая сетка без устойчивого
выигрыша. Постоянная GPU-память resident выше, пик logical buffers ниже.
Raw, независимый анализ, снимок и package checks сохранены; детали и ограничения
в DEVELOPMENT.md. Повтор на другом запуске и оборудовании остаётся в P3.

### P1. Закрыть память и длительный lifecycle

- [ ] Сравнить стоимость ограничения Rust-арены с зафиксированным старым
  алгоритмом при росте и повторном использовании блоков.
- [ ] Измерить JS heap, RSS, WASM memory и GPU allocations раздельно в
  контролируемом процессе; проверить плато после прогрева и пики реаллокации.
- [ ] Расширить churn на одновременно живые roots, DOM и GPU-сцены, несколько
  cores с общим device, отмену, reset и device loss. Проверить callbacks/finalizers
  и суммарный бюджет индексов/временных копий, а не только лимит одного массива.

Готово, когда измерения показывают ограниченный рост для явно заданной нагрузки
и teardown возвращает живые ресурсы к ожидаемой базе.

Промежуточное evidence P1: rolling 64 одновременно живых roots на двух cores,
160080 созданий / 640320 освобождённых WeakRef-объектов; graph capacity и WASM
стабильны, teardown возвращает nodes/edges/jobs к нулю. Raw:
`benchmarks/live-roots-default-isolated-frame-2026-11-01.json`.
RSS и heap выросли, поэтому пункт плато остаётся открытым. Журнал теперь ограничен 32 samples + baseline и четырьмя агрегатами.
Прогон без concurrent recompilation: 1.6 млн roots / 6.4 млн объектов за 73 с,
RSS по четвертям 45.93 / 47.19 / 45.53 / 44.10 МБ. Raw:
`benchmarks/live-roots-ring-no-concurrent-100k-2026-11-01.json`.
Обычный Node снова не освободил четыре объекта за десять GC; отказ сохранён.
Retainer path раннего отказа подтвердил module generator измерителя;
поздние snapshots показывают только WeakRef keep-during-job, после snapshot
и одного GC объекты освобождаются. Исходные отказы сохранены, gate не ослаблен.
Выполнен отдельный timed soak 120 с в обычном Node: 5.91 млн roots /
23.65 млн объектов, final backlog=0, максимум post-GC backlog=41 / возраст44 мс.
Heap по четвертям 8.121 / 8.168 / 8.172 / 8.172 МБ; RSS 53.71…54.58 МБ.
Raw: `benchmarks/live-roots-soak-120s-2026-11-01.json`.
Actions/сцены проверены отдельным 60-секундным soak: 743680 roots, столько
же успешных и отменённых actions, 5.95 млн WeakRef-объектов освобождены.
Graph/WASM/arena стабильны; RSS 87.80 / 91.41 / 92.37 / 89.97 МБ по четвертям.
Raw: `benchmarks/live-roots-actions-scenes-60s-2026-11-01.json`.
Это local RPC, HTTP и серверная конкурентность остаются открытыми.
Выполнен browser DOM/GPU soak на Metal 3: 60 с, 7194 ticks, 1031 сцена,
28776 updates/DOM events, 1796 snapshots; allocation counters сбалансированы,
финальные графы/буферы/текстуры/credits=0. Арена стабильна после фиксированного
warmup; ранний отказ сохранён. Raw: `benchmarks/dom-gpu-soak-metal-2026-11-01.json`.
Совместный recovery fixture прошёл 4/4 на Metal 3: pending pick + reset одного
core, сохранение соседа, explicit device.destroy(), новый adapter/device и
успешный render/pick обоих cores. Final allocations/графы/credits=0.
Raw: `benchmarks/shared-gpu-recovery-metal-2026-11-01.json`; package gates9/9.
Это явное destroy, не спонтанный hardware crash; повторяемые recovery cycles
остаётся открытым; installed-package hardware recovery ниже уже проверен.
Добавлены BROWSER_MATRIX.md и frozen runner: 11/11 DOM fixtures /
90 assertions прошли в production Chrome 156. Source hashes сверены с worktree.
Raw: `benchmarks/browser-matrix-chromium-2026-11-01.json`.
Router history прошёл 14/14 frozen production; Node SSR → browser resume
8/8 и browser HTTP actions7/7 прошли в dev, Node HTTP4/4 повторены.
Raw HTML/snapshot и server-observed replies сохранены, hashes в
`benchmarks/router-ssr-http-qualification-2026-11-01.json`.
Production browser SSR8/8 и HTTP7/7 установленного архива квалифицированы
следующим этапом ниже; нагрузка/async SSR остаются открытыми.
Независимые сборки текущего движка прошли9/9 в одном document:
shared dispatcher, closed Shadow DOM, teardown/reset isolation. Raw:
`benchmarks/independent-engines-production-2026-11-01.json`.
Это не cross-module tracking/untrack/async propagation.
Installed archive consumer прошёл matrix11/11 /90 assertions и hardware
recovery4/4 на Metal3. Все9 package gates прошли;15 runtime JS побайтно
совпали с dist/package,36 production assets проверены по hashes.
Raw: `benchmarks/installed-browser-qualification-2026-11-01.json`.
Production installed SSR→resume8/8 и HTTP actions7/7 прошли; Node HTTP4/4
и package9/9 повторены. Shared endpoints и installed Node renderer, archive/
runtime/asset hashes связаны в
`benchmarks/installed-production-ssr-http-qualification-2026-11-01.json`.
Остальные9 DOM lifecycle fixtures добавлены: installed production matrix
теперь20/20 /176 assertions, package9/9. Raw:
`benchmarks/expanded-installed-lifecycle-2026-11-01.json`;60 assets/15 runtime JS
проверены независимо. Это explicit captured continuation, не automatic async h().
Node async h/JSX component integration реализована: nested/provider owners
после await, immediate sync DOM, fail-fast, cancellation и concurrent requests.
Rust53, production build, JS1229/86, package9 и HTTP4 прошли; installed browser
async roundtrip5/5 + DOM20/176. Evidence:
`benchmarks/async-components-qualification-2026-11-01.json`.
Async request retention проверен15000 requests /120000 objects: exports и
WebAssembly.Memory освобождены без ручного cache advance. Исправлен hot-cache
retention retired cores, включая late cleanup; installed GC regression300
requests/2400 objects включена в package gate. Build,JS1229/86,package9,HTTP4
прошли. Raw: `benchmarks/async-component-retention-memory-2026-11-01.json`.
RSS вырос; длительное plateau и throughput improvement не доказаны.
Concurrent async soak60с прошёл:24 живых requests,2568 barriers,61632 requests /
493056 collected objects,41088 late refusals. Heap последних трёх окон~36.3МБ,
RSS к концу ниже первого окна; многочасовой bound не доказан.
Короткий installed concurrent GC probe включён в package gate;package9/9.
Raw: `benchmarks/async-concurrent-soak-60s-2026-11-01.json`.
Concurrent native HTTP async SSR/disconnect проверен:240 requests,peak12,
80 success/80 failure/80 disconnect; disconnected writes0, final graph/hooks0.
Installed regression48 requests включена в package gate;package9/9.
Evidence: `benchmarks/async-ssr-http-concurrent-2026-11-01.json`.
Это full HTML до headers, не streaming SSR.
Progressive Node stream protocol реализован: shell/patch/complete, one-slot
backpressure, limits, cancellation/pending next и owned browser receiver.
Build,JS1238/87,package9,HTTP4 прошли; native installed HTTP/browser6/6,
body disconnect/recovery2 checks. Raw:
`benchmarks/streaming-qualification-2026-11-01.json`.
Complete пока дублирует final HTML для coherence; versioning, retention/
throughput, long streams/proxies и Edge остаются открытыми.
Wire v1 реализован: explicit version/sequence/streamId + nested payload;
producer numbers0..N, receiver rejects unknown/legacy/replay/gaps before payload.
Исправлен pre-shell ownership: чужой resume state при protocol failure сохраняется.
Build,JS1243/88,package9,HTTP4,installed browser7/7 прошли. Actual legacy/new
receivers отвергают чужой формат в Node/JSDOM. Evidence:
`benchmarks/stream-v1-qualification-2026-11-01.json`.
Автоматический HTTP negotiation не реализован.
Следующий шаг — stream retention/throughput и portable browser/Edge adapters;
длительное плато и стабильность обычного compiler mode остаются открытыми.
DOM/GPU и actions/сцены требуют отдельных сценариев.

### P2. Закрыть браузерные и async-интеграции

- [ ] Собрать браузерную матрицу: события, перенос DOM между documents,
  models, router, SSR/resumption, настоящие HTTP actions и Shadow DOM.
- [ ] Проверить совместные GPU-сценарии: resize/DPR, очередь, отмена,
  readback, восстановление после device loss; отдельно hardware/software adapters.
- [ ] Определить поддержку async SSR components в `h()`, нескольких копий
  пакета, thenables resources/GPU fallback и capture после пользовательского await.
- [ ] Реализовать и квалифицировать Edge/browser SSR adapters в согласованном
  объёме. Node helper уже реализован; остальные среды остаются открытыми.

### P3. Расширить GPU и измерения приложений

- [ ] Проверить соответствие 2D/GPU projection, picking, clipping/depth,
  невыпуклых и перекрывающихся объектов на реальном renderer.
- [ ] Измерить throughput графов, startup и latency на дополнительных реальных
  сценариях; отделить очередь, вычисления, transfer и readback там, где это
  позволяет API. Не выдавать wall-clock latency за чистое время GPU kernel.
- [ ] Повторить untrack A/B и контроль без untrack: глубокий сценарий улучшился,
  но shallow control ранее регрессировал; стоимость bookkeeping ещё открыта.
- [ ] Измерить lifecycle guards, snapshots, cleanup и adoption относительно
  зафиксированной базы. Сравнения с другими движками проводить на настоящих
  реализациях и одинаковых нагрузках.

### P4. Матрица готовности и доставка

- [ ] Для reactive core, DOM, compiler/Forma, SSR, router, GPU, package и native
  studio записать пользовательские сценарии, критерии приёмки и ссылки на
  актуальные проверки; явно отметить частичное и неподдерживаемое.
- [ ] Исправить найденные дефекты с регрессиями и последовательно выполнить
  необходимые native/build/JS/package/HTTP проверки без конкуренции с замерами.
- [ ] Проверить CI, чистую установку, package archive, exports/types и соответствие
  Rust → embedded WASM → установленный пакет.
- [ ] Подготовить обзор изменений, результатов и ограничений. Commit, merge,
  push и публикация — отдельный запрошенный шаг с сохранением чужой работы.

## Ближайший этап: завершить текущие изменения reactive core

Обновлено 2026-11-01. Следующие пункты выполнять в указанном порядке.
Это незавершённая квалификация текущего кода, перед добавлением новых функций.

- [x] Проверить итоговые изменения lazy computed cache в Rust: отсутствие
  terminal effects должно сохранять инвалидирование вычислений; кеш должен
  учитывать только достижимые узлы своего источника и чужие pending effects.
  Пройдены 53 Rust-теста, включая четыре новые регрессии, и шесть JS-регрессий.
  Production build прошёл, embedded SIMD WASM — 54 320 байт.
- [x] Закрыть регрессию statesView: Rust Vec может переместиться внутри прежней
  WASM memory. Проверить сброс JS-view при выделении signal, computed, effect,
  повторном использовании effect и создании tensor. Все пять сценариев прошли
  в итоговом suite; installed-package consumer проверил перемещение отдельно.
- [x] После последних изменений последовательно выполнить native tests,
  production build, полный JS/WASM suite и девять installed-package проверок.
  Ожидаемый состав JS-suite — 1161 тест / 81 файл; это ориентир состава,
  а не уже полученный итоговый результат. Проверить фактические числа в отчёте.
  Итоговый прогон: 1160 passed, один timeout существующего benchmark-теста
  (6046 мс при лимите 5000 мс), 80 файлов passed из 81. Все новые функциональные
  регрессии прошли, девять installed-package checks прошли. Отдельный benchmark-тест
  затем прошёл без изменения timeout. Повтор полного suite с четырьмя workers:
  те же 1160 passed, только benchmark timeout 5994 мс.
  После исправления достоверности отчёта полный suite прошёл: 1161/1161,
  81/81 файлов, maxWorkers=4, 47.70 с; timeout и нагрузки не ослаблены.
  Артефакт: `benchmarks/core-suite-verified-2026-11-01.txt`. Ранние отказы сохранены;
  стабильность времени при разной нагрузке машины этим не доказана.
- [x] Проверить освобождение 64 отдельных cores после reset внутри активных
  observer frames скриптом `scripts/check-reset-core-gc.mjs`: все освобождены
  за один GC при отключённой concurrent recompilation. Артефакт:
  `benchmarks/reset-core-gc-2026-11-01.json`; RSS этим не проверяется.
- [x] Подготовить одинаковые baseline/candidate через
  `scripts/prepare-untrack-bench.mjs`: обе версии должны включать исправления
  корректности, различаться только регистрацией observer contexts.
- [x] Измерить `untrack` на глубинах 1/32/128/256 через
  `scripts/bench-untrack-cores.mjs`; отдельно выполнить контроль с
  `--untracks 0`. Сохранить raw results, хеши и параметры. Ранние замеры
  до исправления корректности не использовать для заявлений об ускорении.
  Первый парный прогон и контроль сохранены в `benchmarks/untrack-core-depth-2026-11-01.json`
  и `benchmarks/untrack-core-control-2026-11-01.json`. Глубокий repeated-untrack
  ускорился; shallow control показал регрессию. Повторяемость и стоимость
  bookkeeping без untrack остаются открытыми при фоновой нагрузке машины.
- [x] Обновить DEVELOPMENT.md итоговыми результатами, отказами и границами
  проверенного. Проверить `git diff --check`.

Тесты и CPU-бенчмарки запускать последовательно, без конкуренции за CPU.
Для полного suite явно фиксировать workers (проверены 1 и 4); существующий timeout
сравнительного benchmark-теста не увеличивать.

## Очерёдность дальнейшей работы

1. Завершить текущий замер GPU-приложения (P0); этап reactive core ниже уже проверен.
2. Доказать границы памяти и длительного lifecycle (разделы 1–2).
3. Собрать браузерную матрицу и закрыть async ownership (разделы 3 и 5).
4. Развить GPU-resident execution и проверить совместно с renderer (раздел 4).
5. Измерить реальные приложения, собрать матрицу готовности и подготовить
   доставку (разделы 6–8).

Каждый пункт закрывать конкретным сценарием, воспроизводимой проверкой
и сохранённым результатом. Результаты отдельных микробенчмарков не закрывают
производительность приложений; bounded memory checks не закрывают RSS.

## 1. Закончить проверку ограниченного роста Rust-арены

- [x] Проверить ограничение резервируемой ёмкости, амортизированный рост,
  сохранение данных и отказ без изменения старого хранилища: 48 Rust-тестов.
- [x] Пересобрать SIMD WASM и production-пакет с текущим кодом.
- [x] Прогнать полный JS/WASM suite, установку пакета и настоящий HTTP transport:
  1101 тест / 78 файлов, 9 проверок пакета и 4 HTTP-теста прошли.
- [ ] Измерить обычный рост и повторное использование свободных блоков;
  отдельно оценить стоимость ограничения ёмкости.
  Регрессия повторного использования пройдена: 2000 циклов по 128 блоков
  разного размера, случайный детерминированный порядок освобождения,
  стабильные адрес/ёмкость арены после прогрева. Сравнительный замер ещё открыт.
  Добавлен локальный WASM-замер: 4096 выделений при росте и 448 000 пар
  выделения/освобождения, семь замеров времени и памяти, проверка reset.
  Результаты: `benchmarks/arena-churn-2026-11-01.json`. Сравнение стоимости
  ограничения со старым алгоритмом остаётся открытым.
- [ ] Проверить давление памяти в контролируемом процессе. Ограничение Vec
  не доказывает ограничение RSS, пиков реаллокации или WASM linear memory.

## 2. Проверить длительную работу и освобождение памяти

- [ ] Циклически создавать и уничтожать roots, effects, resources, actions,
  captured contexts и сцены; проверить выход на плато после прогрева.
  Пройдены 20 000 циклов на двух чередующихся WASM cores без reset:
  живые nodes/edges/pending jobs равны нулю после каждого teardown,
  ёмкость графа и WASM memory стабильны. JS/RSS-плато ещё не доказано;
  записи находятся в `benchmarks/lifecycle-churn-20k-2026-11-01.json`.
  Повторный прогон ограничивает журнал 32 текущими и двумя базовыми
  замерами; WeakRef подтвердил освобождение 180 000 объектов за 20 000 циклов.
  Артефакт: `benchmarks/lifecycle-retention-20k-2026-11-01.json`.
  Этот сценарий не закрывает память DOM, GPU и одновременно живых roots.
  После исправления multi-core tracking повторены 20 000 циклов / 180 000
  объектов: обычный Node потребовал до десяти GC, без concurrent recompilation
  — один. Два более ранних отказа и путь удержания через VM-root сохранены
  в `benchmarks/multicore-gc-diagnostic-2026-11-01.json` и связанных файлах.
  Стабильное время сборки и общее RSS-плато остаются открытыми.
  Новый async-owner сценарий прошёл 1000 циклов / 3000 поддеревьев внутри одного
  живого запроса без reset. Все 15 000 объектов из WeakRef-проб освобождены,
  scopes и внешние abort hooks отсоединены, nodes/edges/jobs возвращаются к нулю.
  После прогрева capacity=6 и WASM memory=1 179 648 байт стабильны. Обычный Node:
  до четырёх GC-раундов; без concurrent recompilation: один. Raw results:
  `benchmarks/async-owner-churn-default-1000-2026-11-01.json` и
  `benchmarks/async-owner-churn-1000-2026-11-01.json`. RSS вырос в обоих прогонах;
  JS/RSS-плато, параллельная нагрузка и память DOM/GPU этим не доказаны.
- [ ] Раздельно измерять живые узлы, ёмкость графа и free lists, JS heap,
  WASM memory и GPU allocations.
- [ ] Повторить для нескольких cores, общего GPU device, отмены и reset.
- [ ] Проверить удержание finalizer/callback ссылок после завершения операций.
- [ ] Проверить большие входы индексов и координат без опасных выделений памяти.
  Добавлен `maxIndexBytes`: фиксированный лимит одной приватной копии индексов
  (по умолчанию 256 МиБ), общий для конструктора и updateIndices. Guarded tests
  отвергают обе ширины индексов выше лимита и почти MAX_SAFE_INTEGER без
  выделения памяти и чтения элементов. Суммарная/пиковая память ещё не закрыта.

## 3. Расширить проверки в настоящем браузере

- [x] Составить воспроизводимую матрицу событий, переноса DOM между documents,
  models, router, SSR/resumption и HTTP actions: BROWSER_MATRIX.md. DOM runner
  прошёл 11 fixtures / 90 assertions; router/SSR/HTTP помечены для отдельных
  свежих запусков, их полная квалификация остаётся открытой.
- [ ] Проверить WebGPU uploads, readback, rendering, device loss, отмену очереди,
  resize/DPR и Shadow DOM в совместных сценариях.
- [ ] Отдельно записывать результаты software adapter и физических GPU;
  CPU fallback и mocks не закрывают аппаратную квалификацию.

## 4. Развить GPU execution

- [x] Добавить кеширование compute pipelines с корректным device lifecycle:
  общий кеш устройства, ключ WGSL/entry point, LRU 32 записи / 1 МиБ ключей,
  вытеснение при ошибках и очистка при потере/уничтожении устройства.
  Браузерный fixture прошёл 10/10 на Apple Metal 3 (аппаратный адаптер):
  повторное использование, новые данные/workgroups, разные entry points,
  восстановление после ошибок и потери устройства. Замеры приложений,
  другие GPU/backends и аппаратная проверка вытеснения ещё открыты.
  Локальное аппаратное A/B выполнено дважды: задержка пакета из 40 dispatch
  с readback снизилась на 35–38% для 7 floats и примерно 22% для 65 536 floats.
  Это измерение конкретной нагрузки; время кадра приложения ещё не проверено.
- [x] Добавить GPU-resident результаты и цепочки без обязательного readback.
  Реализованы цепочки внутри одного dispatch: до 32 kernels, два чередующихся
  выходных GPU-буфера, один submit/readback и одна публикация. На Metal проверены
  реальные вызовы upload/map/submit, частичная запись и максимальная цепочка.
  Добавлены immutable GPU-handles между вызовами и ветвящиеся графы через
  `dispatchGpuComputeResident`; явный `readGpuResidentTensor` публикует итог.
  Отдельный device budget ограничивает persistent storage (256 МиБ / 1024
  reservations по умолчанию). Renderer interop закрыт отдельным пунктом ниже.
  Аппаратный fixture расширен до 15/15: поздняя ошибка bind group и отмена
  владельцем при ожидании map не публикуют результат, вызывают destroy всех
  временных буферов, закрывают scopes и возвращают кредиты очереди. Следующая
  корректная цепочка успешно выполняется после каждого отказа.
  Измерено сравнение с тремя отдельными dispatch на том же Metal: два прогона
  показали снижение задержки запроса на 56–61%. Входные загрузки 28→12 байт
  на элемент, readback 12→4, submit/map 3→1. Конечный observer вызывается один
  раз в обоих вариантах; замеры времени кадра и постоянных GPU-handles открыты.
  Добавлен outputLength для промежуточных стадий: GPU-only размеры независимы
  от WASM-арены, ограничены безопасным числом байт и лимитами устройства.
  На Metal проверена цепочка 129→1→7 с редукцией и обнулением выросшего хвоста.
  Новый resident fixture прошёл 7/7 на физическом Apple Metal 3: ветвление,
  одно host upload / явный readback, отказ input-writing WGSL, admission,
  ошибка shader, отмена map, reset и device loss. Явный read-only layout
  обеспечивает immutability; pipelines разделены по layout/input count.
  Тест границы 1024 результатов проверяет отказ до выделения и массовый reset.
  Shared lifetime hubs держат один постоянный listener на lifetime-сигнал.
  Пройдены production build, полный suite 1207/1207 (84 файла), девять
  installed-package checks и четыре HTTP-теста. Native/source evidence:
  `benchmarks/resident-qualification-2026-11-01.json`.
- [x] Подключить resident buffers к renderer без host readback на каждом кадре.
  Canvas3D/Viewport3D принимают `gpuVertices`: прямой vertex binding, без копии
  vertex buffer и без чтения host transformedVertices. Topology остаётся от mesh.
  Picking читает GPU snapshot только по клику и использует Rust occlusion picker;
  frame metadata содержит identity исходного GPU-handle. Замена кадра/teardown
  отменяют stale readback. Invalid resident source не подменяется host геометрией.
  Исправлены потеря последнего scheduled кадра при source retirement и скрытие
  первичной GPU-ошибки. Обе регрессии воспроизведены до исправления.
  Пройдены production build, 1221 тест / 85 файлов, девять package checks,
  четыре HTTP-теста, 7/7 аппаратных interop checks в dev и production bundle
  установленного пакета; прежний renderer прошёл 17/17 на Apple Metal 3.
- [ ] Измерить планирование графов/throughput на реальных приложениях.
- [ ] Измерить transfer bytes, время очереди, вычисления, readback и кадра.
- [ ] Проверить соответствие 2D/GPU projection, picking, clipping, depth,
  невыпуклой геометрии и перекрывающихся объектов.

## 5. Закрыть границы асинхронного контекста

- [ ] Определить и проверить ownership отдельных async SSR component subtrees.
  Добавлен Node-only `runWithOwnerScopeAsync`: независимые context/owner после
  await, вложенные и параллельные children, сохранение успешных scopes до teardown
  запроса, немедленная очистка при ошибке/отмене и отказ поздних продолжений.
  Синхронные scopes и captured re-entry имеют приоритет. Production build,
  полный suite 1172/1172 (82 файла), девять installed-package checks и четыре
  настоящих HTTP action теста прошли на Node 22.23.4.
  Node renderToStringAsync теперь автоматически ожидает async h/JSX components;
  qualification: JS1229/86, package9, HTTP4, installed browser roundtrip5/5.
  Browser async renderer, Edge, streaming и cross-copy propagation остаются открытыми.
- [ ] Проверить tracking при третьем приостановленном core и нескольких копиях
  пакета; сформулировать поддерживаемые границы.
  Исправлен третий синхронно приостановленный core: untrack и служебные
  callbacks приостанавливают все существующие observer frames в одной копии
  движка. Новые observers внутри callback продолжают tracking; исключения
  и reset восстанавливаются с учётом поколений. Две независимо собранные копии
  текущего кода прошли9/9 native dispatcher/graph isolation проверок; tracking
  и untrack через границу копий остаются открытыми.
- [ ] Проверить цепочки thenables у resources и GPU fallback.
- [ ] Проверить явный capture после пользовательского await в интеграциях.
- [ ] Отдельно реализовать и квалифицировать Edge/browser SSR adapters.

## 6. Измерить производительность приложений

- [x] Исправить достоверность существующего synthetic benchmark: явно назвать
  локальные имитации, убрать вычисленные из чужого времени SSR-результаты,
  измерить настоящие DOM-обходы и проверить их результат. Это не сравнение
  с настоящими React/Vue/Svelte и не аппаратный GPU-замер.

- [ ] Подготовить одинаковые воспроизводимые нагрузки и сохранить raw results,
  версии, оборудование и настройки сборки.
- [ ] Измерять startup, latency/p95, frame time, long tasks, память и bytes.
- [ ] Проверить стоимость lifecycle guards, snapshots, cleanup и adoption
  относительно зафиксированной базы.
- [ ] Делать сравнения с другими движками только на сопоставимых сценариях.

## 7. Собрать матрицу готовности

- [ ] Для reactive core, DOM, compiler/Forma, SSR, router, GPU, package и native
  studio перечислить конкретные пользовательские сценарии и критерии приёмки.
- [ ] Каждому критерию сопоставить актуальную проверку и артефакт результата.
- [ ] Отметить неподдерживаемое, частичное и непроверенное отдельно.
- [ ] Устранить обнаруженные дефекты с регрессиями; число тестов само по себе
  не является критерием готовности.
  Исправлена повторная проверка lifetime после чтения context-итераторов и tuple
  getters: reset/teardown не запускают callback и не портят свежий owner state.
  До исправления три регрессии падали; после него прошли четыре новые регрессии,
  весь suite 1176/1176 (83 файла), production build и девять package checks.


## 8. Подготовить доставку

- [ ] Проверить CI, чистую установку, архив пакета, exports и TypeScript types.
- [ ] Проверить отсутствие расхождений между Rust, embedded WASM и пакетом.
- [ ] Сохранить чужую работу в репозитории. Commit, merge, push и публикация
  выполняются отдельным запрошенным шагом.

## Команды базовой проверки

Запускать последовательно из корня репозитория; build должен предшествовать
JS-тестам после изменения Rust, чтобы они использовали новый embedded WASM.

```sh
CARGO_TARGET_DIR=/tmp/svet-native-validation cargo test --manifest-path packages/svet/crates/svet-wasm/Cargo.toml
npm --prefix packages/svet run build
npm --prefix packages/svet test -- --maxWorkers=1
npm --prefix packages/svet run test:package
npm --prefix packages/svet run test:actions
git diff --check
```
