# Браузерная матрица Svet

Обновлено 2026-11-01. Статус относится к указанному сценарию и артефакту,
а не ко всему subsystem. Наличие fixture без результата не означает pass.
Текущий DOM-прогон: Chrome156/macOS, production consumer установленного
архива SHA256 `13d64c3f8815b112f40187db206ad617cd50a364ec5f14d227df6f93ba4f5e82`,
отдельный same-origin iframe на каждый fixture. Installed runtime и assets
проверены по hashes;15 runtime JS побайтно совпадали с workspace dist/package
соответствующего проверенного этапа, а не обязательно с будущими изменениями.

## Production DOM: 20 fixtures / 176 assertions

Текущий raw: [expanded-installed-lifecycle-2026-11-01.json](benchmarks/expanded-installed-lifecycle-2026-11-01.json).
Он содержит отдельные строки проверок каждого fixture, UA, DPR, build/source/asset hashes.
Независимый accounting: [expanded-installed-lifecycle-analysis-2026-11-01.json](benchmarks/expanded-installed-lifecycle-analysis-2026-11-01.json).

| Сценарий / критерий | Fixture | Результат Chromium |
| --- | --- | --- |
| Teardown model/event, mount replacement, currentTarget, Shadow DOM retargeting, multiple documents | dom-lifecycle.html | 5/5 |
| Foreign/adopted nodes, identity/listener, reactive replacement, SVG conversion, graph teardown | dom-realms.html | 7/7 |
| Delegated events при adoption/Portal, nested moves, rollback, retained nodes после teardown | event-adoption.html | 10/10 |
| Provider lookup, view-owned subscriptions, nested dispatch, foreign-core tracking | event-context.html | 11/11 |
| Retained DOM, reset, синхронный owner/core и listener teardown | event-lifetime.html | 8/8 |
| Radio/current option values, reactive input types, teardown | model-inputs.html | 9/9 |
| Initial/multiple select, dynamic options и observer teardown | model-select.html | 7/7 |
| In-memory native DataTransfer/FileList, model/type changes и resumption lifecycle | model-files.html | 8/8 |
| Fragment consumption, transactional child replacement, retry и lifetime invalidation | dynamic-children.html | 7/7 |
| Keyed fragment rows, reorder/removal и восстановление после ошибки | for-fragments.html | 7/7 |
| Provider fragments, metadata, shadowing, owner/reset invalidation | context-provider-lifetime.html | 11/11 |
| Attribute binding lifetime и teardown | attribute-lifetime.html | 8/8 |
| Explicit captured reentry после await, providers, reset и owned resources | captured-context.html | 11/11 |
| Current item/index и same-key updates, включая Forma и nested loops | for-current-item.html | 7/7 |
| Fragment construction и rollback/lifetime boundaries | fragment-construction.html | 9/9 |
| h construction, callbacks и invalidation/cleanup | h-construction.html | 11/11 |
| Native model adoption между DOM roots | model-adoption.html | 8/8 |
| Reset при native append/move и в closed ShadowRoot | model-reset-roots.html | 16/16 |
| Model lifetime после graph reset | model-reset.html | 8/8 |
| Reactive style lifetime и teardown | style-lifetime.html | 8/8 |

Это программные native DOM события. FileList fixture использует in-memory files,
не file picker и не загрузку с диска. Iframe isolation и собственные teardown
assertions проверяют разные границы; удаление iframe не является доказательством
освобождения всех JS объектов. Forced browser GC не выполнялся.

## Отдельная аппаратная квалификация

| Сценарий | Evidence | Подтверждено / открыто |
| --- | --- | --- |
| Четыре resident сцены, два cores, shared device, DOM events, resize, явный readback, ротация owners | [dom-gpu-soak-metal](benchmarks/dom-gpu-soak-metal-2026-11-01.json) | PASS 60 с / 7194 ticks; конечные tracked resources нулевые; JS/driver memory plateau открыт |
| Pending picking + reset одного core, сохранение соседа, explicit device.destroy(), fresh-device render/pick | [shared-gpu-recovery-metal](benchmarks/shared-gpu-recovery-metal-2026-11-01.json) | PASS 4/4; не spontaneous hardware crash, не повторяемый длительный recovery soak |
| Mapped/resident приложение, геометрия/пиксели, transfers, A/A | [resident-app-interleaved-metal](benchmarks/resident-app-interleaved-metal-2026-11-01.json) | PASS bounded workload; FPS, другие приложения/GPU и чистое GPU kernel time открыты |

Все эти аппаратные результаты — физический Apple Metal 3, не software adapter.
Другие hardware/backends требуют собственных результатов.

## Отдельные сценарии и оставшееся покрытие

| Область | Доступная проверка | Следующий критерий приёмки |
| --- | --- | --- |
| Реальный History API | router-history.html; [production proof](benchmarks/router-history-production-2026-11-01.txt) | PASS 14/14 в отдельной top-level tab, frozen production; другие browsers и installed package открыты |
| Node SSR → browser resume | [installed production raw](benchmarks/installed-production-ssr-2026-11-01.json) | PASS 8/8: production consumer и Node renderer одного установленного архива; async components/Edge и load открыты |
| Настоящие HTTP actions | [installed production raw](benchmarks/installed-production-http-2026-11-01.json); test:actions | PASS production browser7/7 + Node HTTP4/4; более широкая concurrency/late-response нагрузка и soak открыты |
| Две копии движка | [production raw](benchmarks/independent-engines-production-2026-11-01.json) | PASS 9/9: независимые сборки текущего кода, shared dispatcher, closed Shadow DOM, teardown/reset isolation; cross-module tracking/untrack/async propagation открыты |
| Attributes/style/model adoption/reset, h/fragment construction, captured continuation, current keyed item | Включены в текущие20 fixtures | PASS86 новых assertions; новые пользовательские комбинации, реальные input/IME и durability остаются открытыми |
| Installed package | [matrix raw](benchmarks/installed-browser-matrix-2026-11-01.json), [recovery raw](benchmarks/installed-gpu-recovery-metal-2026-11-01.json) | PASS production matrix11/11 /90 assertions, recovery4/4 на Metal3, production SSR8/8 и HTTP7/7; остальные fixtures открыты |
| Async components / browser/Edge | [Node async roundtrip](benchmarks/async-components-browser-2026-11-01.json) | Node renderToStringAsync автоматически ждёт h/JSX components; installed roundtrip5/5, browser async rendering/Edge/streaming и cross-copy propagation открыты |
| Safari / Firefox | Нет свежего результата этой матрицы | Запуск поддерживаемых сценариев в каждом браузере, отметить различия и отсутствие API |
| Реальный ввод, accessibility, touch/keyboard/IME | Programmatic fixture events | Отдельные пользовательские сценарии и проверка поведения через UI |
| Memory / durability | Bounded native Node и browser allocation probes | Многочасовой soak, retained DOM/JS и driver/process memory отдельно |
| Concurrent Node async components | [60-second soak](benchmarks/async-concurrent-soak-60s-2026-11-01.json) |24 synchronized requests,61632 total,493056 collected objects; explicit overlap, context/effect checks, failure/cancel/late refusal; не HTTP и не browser run |
| Concurrent HTTP async SSR/disconnect | [native transport raw](benchmarks/async-ssr-http-concurrent-2026-11-01.json) |240 requests/peak12,80 unfinished response closures, no disconnected writes; installed48 requests; не browser client, Edge или streaming SSR |

## Воспроизведение

Из корня репозитория:

```sh
node packages/svet/scripts/prepare-browser-matrix.mjs --serve
```

Открыть выведенный URL, нажать Run browser matrix. Runner проверяет каждый
fixture по manifest `tests/browser/browser-matrix-cases.json`: итог X/X,
точное число строк PASS и отсутствие FAIL; failure сохраняется в raw отчёте.
Raw JSON находится в DOM `#raw-results`. Runner останавливается на первом отказе,
не считает оставшиеся scenarios выполненными. Frozen preparer собирает все entries,
сохраняет source/asset hashes и отвергает изменение исходников во время build.
Не удалять прежние raw failures при обновлении результата.


Provenance отдельных router/SSR/HTTP запусков:
[router-ssr-http-qualification-2026-11-01.json](benchmarks/router-ssr-http-qualification-2026-11-01.json).
Содержит router production metadata, hashes TS sources и dist/package JS,
серверных scripts и browser fixtures, SHA256 точных SSR HTML/stateJson.
Browser HTTP raw содержит server-observed stats/finish/forget ответы (12 записей,
без dropped responses). Это диагностические данные loopback fixture.

Для отдельных запусков: `node packages/svet/scripts/prepare-router-history.mjs --serve`,
`node packages/svet/scripts/serve-resumption-browser.mjs` и
`node packages/svet/scripts/serve-action-browser.mjs`. Два последних используют
5189 и запускаются последовательно; после смены сервера открыть соответствующий
fixture заново. SSR требует актуального dist/package. Эти команды не заменяют
отдельную production SSR/HTTP browser сборку.


Для independent-engine fixture:
`node packages/svet/scripts/prepare-event-instances.mjs --serve`.
Он компилирует текущий index.ts дважды в отдельные `/first/index.js` и
`/second/index.js`, сохраняет hashes и обслуживает immutable snapshot на 4205.
Две сборки byte-identical, но browser assertions подтверждают разные функции
signal и разные WASM cores. Исторические adoption A/B variants больше не
используются этим fixture. [Независимая проверка hashes](benchmarks/independent-engines-analysis-2026-11-01.json).


Первоначальная installed-package browser qualification повторила11 DOM scenarios в
изолированном consumer после npm pack/install. Runtime импорты идут через
`svet-engine`; прямые workspace imports запрещены. Для event-context/lifetime
публичный package export используется вместо внутреннего wasm-bridge.ts.
Metadata содержит SHA256 архива, всех15 runtime JS и36 production assets.
Все runtime bytes сверены с workspace dist/package; metadata и asset hashes
повторно проверены после native browser execution. [Qualification](benchmarks/installed-browser-qualification-2026-11-01.json).

Воспроизведение: `npm --prefix packages/svet run test:package -- --keep`, затем
`node packages/svet/scripts/serve-installed-browser.mjs` (или явный consumer path
в первом аргументе). Открыть /browser-matrix.html и /shared-gpu-recovery.html
на выведенном localhost4206. --keep сохраняет archive, isolated installation и
production output; preview не пересобирает их и не включает workspace aliases.
Runner metadata report выполняется до browser gate: наличие metadata не означает
успешного browser execution. Проверенный consumer оставлен в /tmp по пути из
qualification JSON; долговечные raw/screenshots лежат в benchmarks.


Production SSR/HTTP теперь обслуживаются тем же installed preview (4206):
`/resumption-roundtrip.html` и `/server-action-http.html`. Общие endpoints
в `scripts/browser-fixture-endpoints.mjs` используются и dev wrappers, и
configurePreviewServer без dev transform/HMR. Preview импортирует Node engine
из consumer/node_modules/svet-engine/dist/package/index.js. SSR response
содержит provenance архива, всех runtime JS и endpoint source hash.
[Production qualification](benchmarks/installed-production-ssr-http-qualification-2026-11-01.json)
связывает browser raw, exact HTML/stateJson hashes, Node provenance и41 asset hash.
Предыдущие dev results сохраняют исторический статус; production pass относится
к текущим bounded scenarios, не ко всему SSR/HTTP subsystem.


Расширение20/176 прошло на отдельном сохранённом consumer;60 production assets
и15 runtime JS повторно сверены после browser run. Девять package gates прошли.
Предыдущие11/90 results сохраняются как исторические артефакты, а GPU recovery
и SSR/HTTP proofs относятся к своим архивам, указанным в qualification JSON.
Увеличение manifest не переносит старые pass автоматически на новые scenarios.


Новая Node async-component integration квалифицирована отдельно от sync SSR:
[async-components-qualification](benchmarks/async-components-qualification-2026-11-01.json).
Installed Node renderer ждёт function components через h/JSX, держит independent
owners/providers после await и сериализует дерево после завершения nested work.
Browser roundtrip5/5 проверяет cleanup, завершённые HTML без async anchors,
resumption поля, созданного после await, native input и teardown. На том же
новом runtime повторены20 fixtures /176 assertions:
[DOM raw](benchmarks/async-components-browser-matrix-2026-11-01.json).
Это не поддержка async components в browser renderer и не Edge adapter.


После Node async integration исправлен retired request hot-cache retention.
Эта новая cache policy квалифицирована build/JS1229/package9/HTTP4 и GC churn
15000 requests /120000 objects, включая WebAssembly.Memory; installed regression
300 requests /2400 objects включена в package gate. [Retention analysis](benchmarks/async-component-retention-analysis-2026-11-01.json).
Предыдущие browser proofs сохраняют свои archive/runtime hashes. Они не
переименовываются в доказательство нового runtime; текущий этап не запускал
браузер заново. RSS plateau остаётся открытым.


## Progressive stream protocol (отдельно от полного HTML SSR)

[Native installed roundtrip](benchmarks/streaming-browser-qualified-2026-11-01.json)
прошёл6/6: shell приходит до release pending component, затем patch/complete,
input DOM identity/state, escaped text, actual writer drain и server/client teardown.
[Installed HTTP](benchmarks/streaming-installed-http-qualified-2026-11-01.json) проверяет body
disconnect после headers и fresh-stream recovery. [Qualification](benchmarks/streaming-qualification-2026-11-01.json)
связывает archive/source/runtime/assets, bytes и stats. Unit9 новых regressions,
полный suite1238/87, package9/9 и native action HTTP4/4 прошли.
Это NDJSON/data-chunk transport с owned native receiver, не raw HTML response.
Final authoritative HTML дублирует дерево; incremental reconciliation, protocol
versioning, retention/throughput, proxy/Edge и cross-browser coverage открыты.
Предыдущие утверждения о том, что full-HTML HTTP proofs не закрывают streaming,
сохраняют силу: этот отдельный новый результат имеет свой archive/hash manifest.


## Wire v1

[Installed v1 browser proof](benchmarks/stream-v1-browser-2026-11-01.json)7/7
добавляет отказ unknown version до payload access/DOM mutation и contiguous
sequence0,1,2. [Qualification](benchmarks/stream-v1-qualification-2026-11-01.json)
содержит17 runtime hashes/64 assets, source и envelope metadata. Полный suite1243/88,
package9/9, action HTTP4/4 и installed stream body disconnect/recovery прошли.
[Legacy/new actual builds](benchmarks/stream-v1-legacy-compat-2026-11-01.json)
проверены в Node/JSDOM: оба отвергают несовместимую форму, сохраняя DOM.
Это не multi-version native browser matrix или автоматический HTTP negotiation.
Unversioned raw proofs выше остаются историческими; v1 использует вложенный payload.

## Negotiated HTML-reference codec, installed production (2026-11-01)

Chrome156 fixture `streaming-roundtrip.html?codec=1` прошёл7/7 через настоящий
Node HTTP endpoint и installed production bundle. Response codec проверяется
до body, decoded records поступают в обычный receiver. Shell остаётся ранним,
complete сохраняет input identity и model12. Bytes248/132403/301, graph0.
Raw: `benchmarks/stream-codec-browser-2026-11-01.json`; hash verification:
`benchmarks/stream-codec-browser-qualification-2026-11-01.json`.
Это один browser/codec happy path, не CORS/mixed-deployment/compression matrix.

## Progressive gzip+codec, installed production (2026-11-01)

`streaming-roundtrip.html?codec=1&gzip=1`: Chrome1567/7 passed. Native fetch
распаковал gzip и доставил shell до release; decoder восстановил records,
input identity/model12 сохранены. Server3 flushes/1 compressor drain/715
compressed bytes/nodes0. Raw: `benchmarks/stream-progressive-gzip-browser-2026-11-01.json`.
Runtime/asset hash check: `benchmarks/stream-progressive-gzip-qualification-2026-11-01.json`.
Не подтверждает large-stream socket saturation или abort во время flush.

## Local workerd DOM lifecycle (2026-11-01)

Текущий Edge fixture использует workerd1.20260730.1, Miniflare4.20260730.0,
LinkeDOM0.18.15 и предварительно скомпилированный WASM. Более ранние записи
«Edge открыт» выше относятся к своим историческим этапам.

Два concurrent fetch events выполняют32 последовательных DOM render cycles:
For reorder сохраняет identity двух rows, индексы обновляются, удалённая row
отсоединяется и освобождает scope; Show заменяет truthy branch на fallback;
SVG namespace и escaping сохраняются; custom event handler вызывается до
teardown и снят после него. Каждый request завершает graph0/cleanup1;
128 row scopes освобождены. Это SSR DOM implementation qualification,
не визуальный browser layout, browser async rendering или доказательство GC.

Raw: [Edge DOM lifecycle](benchmarks/edge-dom-lifecycle-qualified-2026-11-01.json).
Installed optional Edge package gate проверяет ту же matrix и требует counters.
Реальный deployed Cloudflare и другие providers, browser/Edge GC и длительная
DOM нагрузка остаются открыты. Miniflare proxy cancel propagation остаётся
неподтверждённым; host deadline не заменяет эту проверку.

## Edge forms: observed host incompatibility and fix (2026-11-01)

Initial form probe failed in both workerd fetch events: LinkeDOM select.value
has no setter. Negative raw: benchmarks/edge-forms-initial-2026-11-01.json.
Single-select model now updates option.selected rather than assigning value.
Node regression covers getter-only value, duplicate first match, unmatched
selection and change-event roundtrip. Final Edge form matrix passes text input,
checkbox, textarea and single select, reactive updates after await, escaped HTML
and request teardown. Raw: benchmarks/edge-forms-after-2026-11-01.json.
This does not qualify multiple select, native reset/validation or browser events.

## One-way select value (2026-11-01)

Native JSDOM reproduced initial static/reactive value loss: both selected first
option rather than requested b. Corrected by a lifetime-bound post-children
synchronizer and shared option selection logic (also getter-only hosts).
Workerd matrix covers6 controls per concurrent event, including both one-way
select paths. Raw: benchmarks/select-value-edge-2026-11-01.json.
Late option insertion for one-way bindings remains separately unqualified.

## One-way late options (2026-11-01)

Native regressions cover insert/remove/reorder, implicit text and explicit
option value changes, cached binding reads and queued callbacks after unmount
or generation reset. Selection refreshes at a MutationObserver checkpoint.
The workerd fixture now covers7 controls/event and an explicit late option
inserted after await (both events pass). Raw:
benchmarks/select-options-edge-2026-11-01.json. This supersedes the earlier
late-option qualification gap for these cases; browser GC remains unproved.

## Native browser retention GC (2026-11-01)

Chrome156/IAB, production ESM modules served through Vite. Native WeakRef probes:
32 containers behind retained terminal receiver handles (dispose/reset/replacement/
protocol-error),16 disposed select source signals while detached selects remain
held. All48 collect. Four deliberately retained control DOM objects remain alive
before release and collect afterwards; graph0. Repeated twice.

Raw CDP HeapProfiler.collectGarbage was unavailable. Actual mechanism: bounded
allocation pressure,12 task-separated rounds of16MiB byte arrays +500k numeric
array entries, with WeakRef inspection in a later task. This proves observed
collection of the named objects, not forced-GC support, a memory plateau,
SSR request cores, Edge or other browsers.

Raw benchmarks/browser-retention-gc-2026-11-01.json contains both phases,
UA, fixture/index/dom/stream hashes. Initial raw retained separately; its scope
text predates the GC mechanism correction, while gcMechanism records actual
natural GC. Screenshot browser-retention-gc-2026-11-01.png.
Run node scripts/serve-retention-gc.mjs OUTPUT_JSON; open the printed URL;
Allocate → Allocation pressure → Inspect → Release → Allocation pressure → Inspect.

## Attribute-backed Edge multiple select (2026-11-01)

Both workerd events reproduced lost multiple selections (negative raw
edge-multiple-before-2026-11-01.json). LinkeDOM omits multiple reflection and
its option.selected setter enforces single selection. Model now detects the
attribute-backed host, writes selected attributes there and reads selected
values from options, without requiring selectedOptions. Native reflected DOM
continues using option.selected; existing native reset/multiple tests pass.

Final matrix8 controls/event proves initial a,c, reactive b, change event b,c
and both selected options in final SSR HTML, cleanup1/graph0. Raw:
benchmarks/edge-multiple-after-2026-11-01.json. This supersedes the multiple
SSR/change qualification gap, not native reset/validation or deployed Edge.

## Historical failure: eight concurrent workerd timeout streams (2026-11-01)

The initial failure below is superseded by the native-stream ownership fix
and qualification in the next section. Expanded driver with SVET_EDGE_CHURN_MS
(8 concurrent fetch events).
This finds a real qualification failure hidden by the two-event baseline:
`Promise will never complete`, while waiting for timeout reader.closed after
shell. Per-event progress is bounded to16 string-only records; raw failures
include phases. Native control without Svet/DOM/ALS passes8 requests, with and
without an extra watchdog. An independent native timer-promise in the composite
SSR probe makes it pass, but is only an opt-in diagnostic (query keepalive;
SVET_EDGE_KEEPALIVE=1 for churn). Default path has no such timer.

No root cause/fix or broad Edge GC claim is established. Workerd reports
WeakRef and FinalizationRegistry available. Initial failure, stage diagnostics
and native control are retained in benchmarks/edge-lifecycle-churn-*.json and
workerd-native-timer-closed-2026-11-01.json. Earlier two-event qualification
remains bounded to that concurrency; it does not close this new P0.

## Native Web-stream ownership through teardown (2026-11-01)

Adapter now holds the native ReadableStream reference through producer retirement,
clearing it on both fulfillment and rejection. Controlled revert reintroduces
reader.closed failure with the identical worker fixture hash; restoring the
reference passes without watchdog or additional I/O. Exact native GC/heap retainer
path was not captured, so the mechanism beyond lifetime sensitivity remains an
inference. Workerd's ReaderImpl documents detachment clearing its stream ref:
[workerd ReaderImpl](https://github.com/cloudflare/workerd/blob/main/src/workerd/api/streams/readable.h)

Pinned workerd60s /8 concurrent events:1464 events,23424 DOM cycles,
11712 streams,11712 string renders and1464 timeout checks. Native direct HTTP5s:
416 events/3328 streams. Installed archive normal readable churn112 events,
no diagnostic keepalive. Optional package gate now requires that8-event matrix;
SVET_EDGE_CHURN_MS defaults to1000ms (0 is a diagnostic opt-out, rejected by gate).

Node GC:432 objects collected with64 terminal stream/reader pairs still held,
plus16 discarded unstarted streams. These are separate probes, not Edge GC.
Qualification: benchmarks/edge-stream-lease-qualification-2026-11-01.json.
The2026-11-01 workerd override passes timeout churn after the fix but still fails
its full error transport matrix on fetch rejection before Response; not fully
qualified. Edge GC, other providers and multi-hour resource bounds stay open.

## Local workerd request/render token collection (2026-11-01)

Inspector-backed WeakRef probe covers24 requests: complete/cancel/caller abort/
producer error/component timeout/non-settling view timeout, two codecs and two
repetitions. All140 request targets (core, memory, signal, view, capture, and
DOM node where created) collect while24 terminal stream/reader pairs and errors
remain held. Four positive controls stay alive before release and collect after.
HeapProfiler.collectGarbage commands are acknowledged; Runtime.getHeapUsage wakes
inspector processing and console entries are discarded before collection.

Initial pressure-only run was inconclusive because controls failed to collect.
Inspector then exposed4 successful-request cores after release. Snapshot paths:
AsyncContextFrame → closed request token.core; after clearing that field, a second
path remained through renderSession.stream.publish → lifetime closure → core.
Both paths are now retired explicitly. Root-owner cleanup also closes sessions
for never-settling views; closed sessions cannot be repopulated by late errors.

Installed public custom-host probe retains12 request tokens/11 render sessions
and collects all12 cores; ordinary late continuation access still rejects.
A60s inspector-enabled functional churn passes9824 fetch events,157184 DOM cycles,
78592 streams/string renders and9824 timeouts, without diagnostic keepalive.
This is not a paired throughput benchmark or production GC/RSS qualification.

Qualification: [Edge GC](benchmarks/edge-gc-qualification-2026-11-01.json).
Snapshots remain local in /tmp with hashes and reproducible retainer-analysis
commands in the receipt. Owner-channel retained tokens, other providers,
physical RSS/GC scheduling and multi-hour bounds remain open.


### 2026-11-01 — retained owner-channel tokens

Pinned local workerd/LinkeDOM collects all 140 request targets with 20 retired
owner tokens held. Positive controls: 4 held, 0 after release; inspector commands
acknowledged. Installed Node ALS collects 17 cores with all three token channels
held. [Receipt](benchmarks/owner-token-qualification-2026-11-01.json).
This does not qualify native browser async carriers, other Edge providers,
natural GC cadence or multi-hour RSS bounds.


### 2026-11-01 — experimental Miniflare5 beta

Local 5.20261023.3-beta passes matched workerd/date 2026-10-23 and November
workerd/date override, including transport recovery and retained-owner inspector
GC. [Raw](benchmarks/edge-beta-november-2026-11-01.json). Official v4-options
converter is used. Stable July default remains unchanged; beta is experimental.
Remote hosts, natural GC and long RSS bounds remain unqualified.


### 2026-11-01 — native constraint validation

Current-source in-app Chromium: 13/13 validation/model checks passed, including
reactive required/disabled/noValidate, native invalid/submit behavior, custom
validity preservation and teardown.
[Receipt](benchmarks/model-validation-qualification-2026-11-01.json).
Installed production, additional browsers and native Edge DOM validation remain open.


Installed production follow-up: complete 21-fixture DOM/event/model matrix
passed 189 assertions, including validation13 and reset/root cases.
[Raw](benchmarks/model-validation-installed-matrix-2026-11-01.json).
Current-source fixture is now included in subsequent package consumer builds
through browser-matrix-cases.json. Single in-app Chromium only.


### 2026-11-02 — Edge cross-copy async owner/context

Pinned local workerd/LinkeDOM and experimental beta-controller/November binary
profiles pass two independent engines,2 concurrent host events/8 request cores,
owner/context after await, child cleanup/refusal, captured root precedence and
concurrent isolation. Inspector collects all8 cores+8 Memory; controls4 held/0
released. [Receipt](benchmarks/edge-cross-copy-owner-qualification-2026-11-02.json).
These checks enter the Edge-enabled package gate. Remote providers, mixed
versions, implicit core propagation and native browser carriers remain open.


### 2026-11-02 — Edge foreign reactive DOM lifecycle

A parent signal from copy A drives copy B's text/attribute after await. Delegated
click restores owner/core/context; child rejection stops DOM updates and events
while the parent signal remains live. Both local profiles pass. Inspector collects
8 cores+8 Memory+2 signals+2 nodes; controls4 held/0 released.
[Receipt](benchmarks/edge-copy-dom-qualification-2026-11-02.json).
Successful-child retained event handlers and native browser events remain open.


### 2026-11-02 — independent-copy built-in boundaries

Chrome156 native DOM: source production bundles4/4 and installed-package
production bundles4/4 passed. Explicit same-core copies in both directions:
Suspense pending/completion/refetch/cancellation, ErrorBoundary rejection/retry,
exactly one recovery mount and graph0. No implicit browser async carrier is
claimed. Other browsers and natural-GC/RSS remain open.
[Raw and screenshots](benchmarks/browser-copy-boundaries-qualification-2026-11-02.json).


### 2026-11-02 — cross-copy router on native DOM

Chrome156 installed production independent bundles:3/3 passed. RouterView exposes
router identity to foreign useRouter/Link in both directions; native click is
intercepted, URL fields and route params match, prior route cleans once, retired
Link becomes inactive and graph0. Non-ambient-core navigation publishes one
consistent URL update. Actual cross-copy back/forward concurrency and guard
rollback remain open; this fixture does not qualify them.
[Raw/screenshot](benchmarks/browser-copy-router-qualification-2026-11-02.json).


### 2026-11-02 — cross-copy native history observers

Chrome156 installed bundles5/5: prior router cases plus both-direction actual
Back/Forward, denied Back compensation, redirect round trips, exact foreign
observer publication sequence, host state preservation and graph0. One active
router per scenario. Competing routers/rapid traversal bursts remain open.
[Raw/screenshot](benchmarks/browser-copy-history-qualification-2026-11-02.json).


### 2026-11-02 — cross-copy history bursts: partial

Six denied burst patterns pass in each direction in separate fresh tabs
(11/11 cases per tab). Combined17-case run failed setup of the last case after
16 passed; cause unresolved. The combined run remains open and is not replaced
by the smaller passing runs.
[Evidence](benchmarks/browser-copy-burst-qualification-2026-11-02.json).


### 2026-11-02 — full burst matrix with explicit pacing

Chrome156 installed bundles17/17 with optional inter-case pacing, all original
burst patterns/deadlines retained. Native-only201-command control reproduces
missing final event; fixture cancels default navigation of retired test Links.
Unpaced continuous16/17 refusal remains open; paced pass does not replace it.
[Evidence](benchmarks/browser-copy-burst-controls-2026-11-02.json).


### 2026-11-02 — retired UI core/memory collection

Chrome156 installed independent copies:16 cores+16 Memory collected with40
retained terminal UI references; controls4 held/0 released. Natural GC under
bounded allocation pressure, no forced/CDP collection. Same runtime passes17/17
native history cases with explicit pacing. Idle cadence/RSS, browser async SSR,
Edge and other browsers remain open.
[Raw/screenshots](benchmarks/retired-ui-browser-qualification-2026-11-02.json).


### 2026-11-02 — local Edge retained UI callback GC

Stable and beta/November workerd:32 core/Memory targets collect with40 retained
UI references over16 requests; same probe on previous runtime retains32/32.
Controls4 held/0 released, inspector GC commands acknowledged. Installed gate
requires exact accounting alongside legacy140 and cross-copy24 targets. This is
LinkeDOM/local forced-GC qualification; deployed/natural cadence and native Edge
History API remain open.
[Evidence](benchmarks/edge-retained-ui-qualification-2026-11-02.json).


### 2026-11-02 — live route-table replacement through foreign RouterView

Chrome156 installed production bundles19/19 (paced). Same fixture before:
17 pass/2 replacement cases fail; after:both copy directions render replaced
tables correctly, prior route cleanup1, params/DOM correct, terminal matcher
avoids disposed options getters and graph0. Continuous unpaced stress remains
open. [Raw/screenshot](benchmarks/router-table-browser-qualification-2026-11-02.json).
