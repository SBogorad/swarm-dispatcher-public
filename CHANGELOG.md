# Changelog

Все значимые изменения проекта фиксируются в этом файле.
Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
версии следуют [Semantic Versioning](https://semver.org/lang/ru/).

---

## [Unreleased]

Планируется v0.6: Distributed Worker MVP для batch-задач на разных машинах
или виртуальных машинах. Критерии и ограничения описаны в
`docs/DISTRIBUTED_WORKER_MVP.md`.

Release-tag требует acceptance harness с `--full-workspace-test`, строгим
`release_gate: STRICT`, нулевым `blocked_count` и валидным attestation.

### Added (миграционный раннер для живого тома — §5.3)

- **`scripts/apply_migrations.sh`** — раннер миграций для УЖЕ СУЩЕСТВУЮЩЕГО тома
  (ночная смена 2026-10-17). Реестр применённого — `public.schema_migrations`
  (filename, sha256, время, кто применил); тот же формат пишет initdb-скрипт
  `scripts/05-migrations.sh`, поэтому «свежий том» и «живой том» говорят на
  одном языке. Каждый файл применяется в отдельной транзакции ВМЕСТЕ с записью
  в реестр; есть `--status`/`--dry-run`, advisory-lock, отказ при дрейфе
  контрольной суммы, отказ на неинициализированной БД и явное «усыновление»
  унаследованного тома (`--adopt-up-to <префикс>`).
- **Почему это было дефектом, а не удобством:** миграция 014 (фикс identity в
  перекрывающих миграциях, FIND-NEW-02) после мержа не доехала до демо-тома —
  `task_rework_feedback` осталась без `assert_worker_identity` (проверено на
  живом PG). Initdb-скрипты на непустом томе пропускаются, а штатного пути
  накатить миграции на живой том не было — только ручной `psql -f` по порядку,
  без реестра и без защиты от повторного/частичного применения. Раннер
  применён к демо-тому: 001–013 усыновлены (`--adopt-up-to 013_`), 014
  накатана; `task_rework_feedback` теперь отдаёт `worker_identity_mismatch`
  чужому воркеру (до этого — тихо возвращала данные).
- **Страж** `tests/test_migration_ledger_runner.py`: статически требует, чтобы
  форма реестра у двух путей совпадала, чтобы миграция и запись в реестр были в
  одной транзакции, чтобы шаг был подключён в гейт; поведенчески — накат,
  no-op, дрейф, отказ на унаследованном томе и откат упавшей миграции без
  записи в реестр. Мутации M1–M7 краснеют (M7 — возврат `psql -f <хостовый
  путь>` вместо stdin: в `--docker-exec` режиме psql исполняется внутри
  контейнера, а `mktemp` живёт на хосте; поймано на живом демо-томе).
- `docs/KNOWN_LIMITATIONS.md` §5.3: ограничение переведено в «закрыта частично»
  — раннер есть, автоматического применения при старте сервиса по-прежнему нет.

### Fixed (безопасность — identity воркера в перекрывающих миграциях, FIND-NEW-02)

- **Утечка диагностики ревью чужой задачи** (ночная смена 2026-10-17).
  `task_rework_feedback` (миграция 012) проверяла только `assert_lease`, а тот
  сверяет аренду с ПЕРЕДАННЫМ `p_worker`, не с `session_user`. Значит
  `swarm_worker_w1`, зная `(task_id, epoch)` живой аренды `w2`, читал `rework`,
  `verdict`, `reviewer`, `artifact_ref` и `checks` чужой задачи (доказано на
  живом PG). Тот же класс, что FIND-NEW-01 (`fail_task`/`fail_task_state`).
  Фикс — миграция `schema/migrations/014_identity_in_override_migrations.sql`
  (identity первой содержательной строкой, до `assert_lease`).
- **Слепой зон арбитров:** снятие identity-строки из финальных тел `claim_task`
  и `complete_task` (миграция 011) переживало весь набор арбитров — зонд на
  disposable-БД дал `SURVIVED` для обеих мутаций. Добавлены поведенческие
  арбитры «чужой воркер → `WK003`» и позитивные контроли для `claim_task`,
  `complete_task`, `save_artifact`, `heartbeat`, `task_rework_feedback`
  (`tests/test_worker_identity_f3.py`).
- **Новый статический страж** `tests/test_migration_identity_preserved.py`:
  identity выживает перекрытие; обёртка воркера с арендой обязана её иметь;
  identity стоит ДО `assert_lease`. Подключён в `scripts/acceptance_harness.sh`.
- **Мутации S12–S14** (`scripts/run_mutations_sql.sh`): снятие identity в
  перекрывающих миграциях 011 (`claim_task`, `complete_task`) и 014
  (`task_rework_feedback`) — каждая обязана валить арбитры.

### Fixed (документация — дрейф ссылок и карта репозитория)

- **Реестр ограничений приведён к коду** (PR #63). `docs/KNOWN_LIMITATIONS.md`
  числил F9 и F18 открытыми, хотя оба закрыты `5e27a4c` (PR #43). Ссылки
  пересчитаны по коду: `worker.rs:160`→`:88` (`thread::sleep` убран из цикла
  heartbeat, точка входа — `sleep_interruptible`), `parse.rs:104`→`:163`
  (`deterministic_root_id`), плюс два документа, уехавших в `docs/optional/`,
  и один переименованный скрипт ролей. F10 раскрыт в доках.
- **Страж дрейфа ссылок в реестре** (PR #63):
  `tests/test_known_limitations_refs.py` — курируемый набор анкеров; ссылка
  обязана указывать на строку с ожидаемым символом, иначе тест краснеет и
  требует обновить документ, а не тихо разойтись с кодом.
- **Полный скан живого корпуса документов** (PR #65). 34 битые ссылки:
  десять документов уехали в `docs/optional/`, а ссылки остались на прежний
  путь; интеграционный тест ядра переименован в
  `crates/swarm-core/tests/integration_verified.rs`; путь монтирования внутри
  контейнера стоял вместо репозиторного (`scripts/02-roles.sh`). Утраченные
  артефакты верификации помечены как утраченные, а не оставлены молчаливой
  ссылкой в никуда.
- **Страж дрейфа по всем живым документам** (PR #65, расширен в #66):
  `tests/test_docs_link_drift.py`. Скоуп — корневые `*.md` (кроме снимков
  `REPORT_*.md`) и `docs/**` без замороженных `docs/history/**` и
  `docs/audit/**`; живой индекс `docs/history/README.md` внутри. 24 законные
  битые ссылки внесены в `ALLOW` с причиной, и тест требует, чтобы запись
  уходила, как только ссылка починилась или исчезла, — иначе allowlist
  незаметно превращается в свалку.
- **Второй класс ссылок — без бэктиков, от корня репо** (PR #66). Здесь
  нашлась самая неприятная гниль: `README.md` (точка входа) вёл в четыре
  документа, уехавших в `docs/optional/`, и в `PLAIN_EXPLANATION.md`,
  которого не было никогда. Путь проверяется, только если его первый
  компонент — элемент корня репозитория (список берётся из файловой системы):
  короткое сокращение пути внутри описания конкретного крейта проверке не
  подлежит — ложные срабатывания в drift guard хуже пропусков.
- **`MANIFEST.md` переписан по факту** (PR #67). Карта репозитория
  перечисляла ~12 несуществующих файлов (в том числе документы, уехавшие в
  `docs/optional/`, и крейт веб-доски `crates/swarm-web/`, которого нет) и
  молчала о существующих (`swarm-llm`, `swarm-planner`, `swarm-reviewer`,
  `schema/migrations/`, `step2/`, `step2b/`, `demo/`, `ops/`, `sandbox/`,
  `docs/ci/`, `docs/research/`).
- **Статус вопросов внешнему аудитору** (PR #68): вопрос 16 («дрейф ссылок»)
  закрыт по факту; отмечено, что F10 и «миграции только на пустом томе»
  остаются открытыми.
- **Оба стража подключены в гейт** (`scripts/acceptance_harness.sh`) —
  дрейф ловится CI, а не аудитом. Оба статические, БД не требуют.

### Fixed (техдолг — `cargo fmt --check` красный на HEAD + шаг форматирования в гейте)

- **Форматирование приведено к канону rustfmt.** `prompt.rs` и `worker.rs`
  (`crates/swarm-worker/src`) были не отформатированы на `master`: PR #38
  привёл HEAD к канону, но последующие #45/#51 внесли новый код без прогона
  rustfmt. Изменений семантики нет — `cargo check --workspace --locked` зелёный.
- **Гейт теперь проверяет форматирование.** Шаг `fmt_check` в
  `scripts/acceptance_harness.sh`: `cargo fmt --check`, fail-closed (rustfmt
  отсутствует → `BLOCKED`, не «зелено»). Причина — класс «проверки, которой
  нет»: CI ставил компонент rustfmt, но шага не было ни в CI, ни в harness,
  поэтому расхождение жило незамеченным.
- **Отдельно учтено:** `cargo fmt` без установленного rustfmt завершается 0,
  ничего не проверяя, — поэтому шаг сначала проверяет `cargo fmt --version`
  (иначе проверка была бы «зелёной всегда»). Версии не флейки: rustfmt 1.94.1
  (пин CI) и 1.99.0 (локальный) дают байт-идентичный вывод на этом дереве.
- **Проверка проверки поломкой** (Принцип 3): намеренно испорченный файл →
  `FAIL`, чистое дерево → `PASS`, rustfmt недоступен → `BLOCKED`.

### Fixed (F8 — бюджет не ограничивал реальные расходы, KNOWN_LIMITATIONS §5.10)

- **Синхронный учёт расхода.** Триггер `AFTER INSERT ON llm_calls`
  (`llm_calls_spend_sync`) атомарно увеличивает `tasks.spent_tokens`. Раньше
  расход рос только в `complete_task`: воркер, оплативший вызов LLM и упавший
  до `complete`, оставлял расход видимым в ledger и невидимым в бюджете.
- **Единый источник истины.** `complete_task` больше не прибавляет `p_tokens`
  повторно (иначе двойной счёт). Совместимость: если строк ledger по задаче
  нет, `p_tokens` учитывается ровно один раз (старый контракт вызова, тесты).
- **Двойной бюджетный гейт.** Колонка `tasks.root_budget_cap` (суммарный
  бюджет дерева) + проверка `SUM(spent_tokens) WHERE root_id = X <
  root_budget_cap` в `claim_task`, `complete_task` и в воркере перед вызовом
  LLM (`task_budget_state`).
- **Бюджет-осознанный реклейм.** `reclaim_expired` при исчерпанном бюджете
  (локальном или по дереву) переводит задачу сразу в `budget_exhausted`, а не
  в `ready` — цикл повторных оборотов больше не тратит деньги мимо бюджета.
- **Миграция** `schema/migrations/011_budget_sync_and_root_cap.sql`;
  `schema/schema_v0.sql`, `step2b/roles_2b.sql`, `step2/roles.sql` приведены в
  соответствие. Арбитр — `tests/test_f8_orphan_llm_spend.py` (краснеет на
  старой логике, зеленеет на фиксе).

### Added (Issue #1: архитектурные проверки ast-grep)

- **`ast-grep` вместо регулярных выражений.** Три правила в `.ast-grep/rules/`,
  конфиг `sgconfig.yml`. AST-точность: `no-unwrap-in-production` ловит
  `.unwrap()` в production-коде, но не трогает `#[cfg(test)] mod tests` и
  интеграционные тесты/примеры (`files: **/src/**/*.rs`).
- **Фикстуры к каждому правилу** (`.ast-grep/tests/*-test.yml`): `ast-grep test`
  проверяет сами правила (валидный код не должен срабатывать, невалидный — должен).
  Фикстура сразу вскрыла дыру: одиночный паттерн `$FIELD = "$VALUE"` не матчил
  `let`-биндинг — `no-hardcoded-secrets` переведён на `any` из `let`/`const`/
  `static`/field-инициализатора.
- **Встроено в оба гейта.** Локально — `scripts/acceptance_harness.sh` (шаги
  `ast-grep test` + `ast-grep scan --error`; без инструмента — BLOCKED, не «зелёно»);
  в CI — `.github/workflows/release-gate.yml` ДО дорогой сборки (fail-fast).
- **Версия зафиксирована** (`@ast-grep/cli@0.50.0`) — иначе правила и фикстуры
  разойдутся с CI.
- **Ограничение (проверено):** `ast-grep-ignore` уважается только в `scan`,
  но НЕ в `ast-grep test` — в фикстурах исключения не проверяются.
- Подавления оформляются как `// ast-grep-ignore: <rule-id>` (обязателен id:
  голый `ast-grep-ignore` — ошибка `no-suppress-all`), а объяснение — отдельной
  строкой выше (текст после двоеточия трактуется как id правила).

### Added (Issue #7/#10: выбор модели по цене успешного результата)

- **Модель выбирается по измерению, а не по названию.** Дешёвая модель, которая
  валится в `max_rework`, дороже дорогой, которая сдаёт с первого раза. Селектор
  `crates/swarm-worker/src/model_selector.rs` берёт минимум
  `cost_per_success_usd` — цены **успешного** результата.
- **Каталог цен из живого прайса.** `scripts/fetch_model_catalog.py` тянет
  `GET /api/pricing` и считает USD за 1M токенов:
  `input = model_ratio * group_ratio * 2`, `output = input * completion_ratio`.
  Формула сверена с per-call логом `/api/log/token` (`quota / 500000` совпадает
  с ledger) — то есть цены не выдуманы, а подтверждены фактом.
- **Статистика качества.** View `llm_model_quality` (миграция 010) считает по
  модели `tasks`, `tasks_done`, `success_rate`, `cost_per_success_usd`,
  `avg_rework` по терминальным задачам; `scripts/model_quality_report.py`
  превращает её в JSON для воркера.
- **Уровни задач.** Контракт может задать `quality_tier`; задача уровня `T`
  уходит только модели с `quality_tier >= T` — слабая модель на сложной задаче
  жжёт ретраи и дороже мощной.
- **Выключено по умолчанию.** `SWARM_MODEL_AUTOSELECT=0` — модель берётся из
  `LLM_MODEL`, как раньше. Любая проблема с каталогом/статистикой не роняет
  воркер: деградация на одну модель с `warn`.
- Юнит-тесты селектора (11) и каталога/отчёта (15, включая DB-тесты view).

### Added (Issue #4: один агент — один контролируемый проход)

- **Жёсткие пределы прохода воркера.** `MAX_LLM_CALLS` (максимум LLM-вызовов
  за проход, включая ретраи) и `MAX_DURATION_SEC` (максимум секунд с начала
  прохода). Проверяются ПЕРЕД очередным claim; `0` — без лимита (штатный
  долгоживущий воркер не меняет поведение).
- **`RUN_ONCE=1`** — одна задача и выход. Контролируемый прогон роя на одной
  задаче без бесконечных повторов; повторный запуск — осознанное решение
  оператора после разбора причины.
- Счётчик LLM-вызовов в `WorkerConfig` (`Arc<AtomicU32>`) считает каждый вызов,
  включая ретраи и неудачные — иначе ретраи обходили бы лимит.
- Юнит-тесты `RunLimits` (граница `made == limit`, приоритет вызовов над
  временем, `0` = без лимита) + интеграционные тесты счётчика (с ретраями и
  без).

### Added (Issue #3: hard budget guard)

- `MAX_TOTAL_COST_USD` — глобальный лимит расходов из cost ledger, проверяется
  ПЕРЕД каждым LLM-вызовом; при недоступном ledger — fail-closed (не тратим).
- Circuit breaker на HTTP 401/403: повтор бессмысленен, задача → `failed`.
- `MAX_CONSECUTIVE_ERRORS` — самоотключение воркера после N неудач подряд.

---

## [0.5.2] — 2026-10-11 (в работе)

Hardening по итогам трёх внешних аудитов v0.5.1 и прогонов штатного harness.

### Security

- **Строгий whitelist подкоманд git/cargo** (`f474e97`). F11 проверял только
  ОПЦИИ, поэтому `git config` (подкоманда) проходил насквозь — а
  `git config core.pager "sh -c ..."` / `git config alias.x '!...'` дают RCE.
  Переход от blacklist опций к deny-by-default whitelist подкоманд
  (`ALLOWED_GIT_SUBCMDS`/`ALLOWED_CARGO_SUBCMDS`); `cargo xtask` тоже закрыт.
- **Запрет записи cargo-конфигурации** (`bf191f4`). `sandbox::write_file`
  отклоняет пути с компонентом `.cargo` и файлы `config.toml`/`config`:
  иначе агент подменял `[build] rustc = "..."` и обходил allowlist (вектор
  идёт мимо argv).

### Changed (ломающее изменение контракта)

- **`submit_review`: каждая проверка обязана возвращаться с `id`.** Если у
  проверки в контракте был `id`, рецензент ДОЛЖЕН вернуть тот же `id`; если
  `id` не было — совпасть должны `type` и `run`. Ранее фикстуры без `run`
  проходили гейт; после ужесточения это привело к регрессии фикстур
  `tests/test_v0.py` (закрыто `b9ea4d0`, Волна 4). Клиентам рецензента,
  возвращающим checks без `id` и без `run`, требуется обновление.

### Added

- **`ops/db_backup.sh`** — dump/restore/verify с контрольной суммой и
  проверкой восстановления в disposable-БД (`verify_chain` по задачам).
  Закрывает практическую часть ограничения 5.1; шаг `backup/restore` добавлен
  в acceptance harness.
- **`ops/docker-socket-proxy.md` + `ops/docker-compose.shell-tools.yml`** —
  socket-proxy с белым списком Docker API (CONTAINERS/EXEC/IMAGES/INFO/PING)
  вместо прямого монтирования `/var/run/docker.sock`. Не включено по умолчанию.
- **CI job `mutations`** (`.github/workflows/release-gate.yml`) — мутационное
  тестирование M1–M6, fail on survivors; выживший мутант теперь валит CI.
- **Fail-closed `check_integration_not_ignored`** — гейт не может «пройти»,
  не выполнив интеграционные тесты.
- Тесты слоёв allowlist: L1/L7/L8/L9/L10/L11/L12 (мутационно проверены,
  выживших нет) + поведенческий арбитр whitelist подкоманд (`56c317b`).
- Артефакт `harness-*.tests.txt` и поле `test_names` в attestation —
  конкретные имена тестов, а не только названия шагов (`56da946`).

### Fixed

- **7 дефектов тест-инфраструктуры** (найдены ночным прогоном): SIGPIPE+pipefail
  в FIND-01; `run_mutations_v04.sh` без `--ignored` (4/6 ложных выживших); гейт
  игнорировал 21 интеграционный тест; устаревшая копия `integration.rs`;
  doc-блок `ignore` с псевдокодом; W4-арбитры не запускались (38 за BLOCKED);
  их же не изолированность (lease_lost).
- **FIND-01 в harness был вакуумным** (`bf191f4`). Негативная проверка искала
  `swarm_worker::sandbox` в форме `::`, но rustc мангалит как
  `swarm_worker7sandbox` → grep всегда давал 0, шаг PASS-ил при реальной
  утечке. Теперь: положительный контроль (строка runtime-гейта обязана быть
  в бинаре) + негативный в реальной legacy-форме мангалинга.

---

## [0.5.1] — 2026-10-11

Hardening по итогам аудита v0.5.0 (три независимых ревьюера + штатный harness).

### Security

- **Runtime-гейт `WORKER_SHELL_TOOLS=1`** (`1051d6a`). Компиляционный бан
  shell-tools снят, но включение стало осознанным: без флага воркер
  отказывает в запуске sandbox ДО обращения к Docker.
- **N3:** `name_prefix` sandbox обязан содержать worker-идентификатор;
  равенство дефолту отклоняется — иначе `cleanup_stale` одного воркера мог
  задеть контейнеры соседа.
- **N1:** `git -C <dir>` больше не ложно-блокируется (короткие флаги
  сравниваются с учётом регистра: `-C` ≠ `-c`).

### Fixed

- **Регрессия F7: тип проверки `unit`** (`92f27a0`). F7 сузил реестр до
  `('cmd','test','lint','manual')`, но `unit` — легитимный тип базового
  контракта `step2/test_review_gate.py`; `add_task` падал с `CT001`. `unit`
  возвращён и в SQL-реестр, и в Rust `ALLOWED_CHECK_TYPES` (N5).
- **conftest блокировал базовые сьюты** (`ee8e007`). Глобальный
  `W4_*`-гейт в `pytest_configure` валил сбор любого `pytest tests/test_v0.py`.
  Гейт сделан селективным (только волна-4-арбитры), честность сохранена
  (громкий BLOCKED-warning, не тихий skip).
- **N2:** `worker_sandbox` требует живых Docker+PG — помечен `#[ignore]`.

---

## [0.5.0] — 2026-10-11

Sandbox hardening, Волна 5 (F6/F7/F11/F12) + снятие compile-gate shell-tools.

- F6: `wrap_untrusted` экранирует `</`/`>` в untrusted-тексте задач.
- F7: реестр допустимых типов проверок контракта (закрыт).
- F11: запрет конфигурационных опций git/cargo (`-c`, `--config`, `-Z`, ...).
- F12: `--pids-limit` в sandbox; префикс контейнера с worker-идентификатором.
- Модули `agent_loop`/`sandbox`/`allowlist`/`audit` компилируются всегда;
  feature `shell-tools` оставлен пустым для обратной совместимости.

---

## [0.4.1] — 2026-10-09

- Agent Loop прерывает DSML batch при первом policy reject; причина отказа
  передаётся LLM для исправления отдельным следующим вызовом.
- DSML/JSON parsing стал строгим: malformed quoted strings, произвольный текст
  вместо JSON и неоднозначные аргументы возвращают `InvalidToolCall`.
- Prompt Agent Loop явно требует native DSML для tool calls и строгий JSON
  `done=true` для финального ответа.
- Перенесён неиспользуемый `step2b/roles_2b_patch_section3.sql` в
  `docs/history/obsolete/`.
- Проверки: 46 unit + 11 Docker integration; реальный DeepSeek Agent Loop —
  4 итерации / 3 tool calls, `cargo test` в sandbox, `verify_chain=true`.

---

## [0.4.0] — 2026-10-08

Shell tools + Docker sandbox (ТЗ `docs/TZ_v0.4_shell_tools_sandbox.md`).

### Added

- **Agent Loop** (`crates/swarm-worker/src/agent_loop.rs`): LLM в цикле
  возвращает tool call (`run_shell`, `read_file`, `write_file`) или финальный
  ответ; лимит итераций `WORKER_MAX_ITERATIONS` (default 20).
- **Allowlist команд** (`allowlist.rs`): deny-by-default, env
  `WORKER_ALLOWED_COMMANDS`, явный deny-список, запрет shell-операторов
  (`>`, `|`, `;`, `&&`, `` ` ``, `$(`), запрет `..`, рекурсивная проверка
  `bash -c "..."` (иначе первое слово `bash` обходило allowlist).
- **Docker sandbox** (`sandbox.rs`): один контейнер на задачу,
  `--cap-drop ALL`, `--read-only`, `--security-opt no-new-privileges:true`,
  `--network none`, tmpfs `/tmp` и `/workspace`, лимиты памяти/CPU, uid 1001.
  Таймаут команды (`WORKER_CMD_TIMEOUT_SEC`, WK005) и лимит вывода 10 MB.
- **Аудит tool calls** (`audit.rs` + `step2/roles_patch_tool_call_audit.sql`):
  событие `kind='tool_call'` в `events` (только hash + размер вывода).
  Пишется через SECURITY DEFINER-функцию `record_tool_call`, потому что
  `append_event` намеренно не выдан никому (журнал нельзя подделать снаружи).
- **Образ песочницы** (`sandbox/Dockerfile`): ubuntu 24.04 + Rust 1.85,
  непривилегированный пользователь 1001.
- **Тесты:** +23 unit (`swarm-worker` 12 → 35), новый
  `crates/swarm-worker/tests/worker_sandbox.rs` (9 integration, реальный Docker).
- **Мутации** M1–M6: `scripts/run_mutations_v04.sh` — 6 из 6 ловятся.

### Changed

- Порядок применения SQL: добавлен пятый файл
  `step2/roles_patch_tool_call_audit.sql` (README, NATIVE_SETUP, compose).
- `swarm-worker`: `postgres` и `uuid` переведены из dev- в обычные зависимости
  (аудит пишется отдельным соединением).

### Removed

- `schema/roles.sql` — заглушка, никем не применялась →
  `docs/history/obsolete/schema_roles.sql.stub`.
- `step2b/test_roles_2b.DELIVERED_partial.py` — обрезанный остаток доставки →
  `docs/history/obsolete/`.

### Security

- `WORKER_SHELL_TOOLS=0` по умолчанию: включение требует Docker-сокета в
  контейнере воркера (= root на хосте). См. `docs/KNOWN_LIMITATIONS.md` §5.6.

---

## [0.1.0] — 2026-10-08

Первый публичный тег. Работающий фундамент: схема БД, Rust-обёртка,
watchdog, worker-заглушка, CLI, Docker Compose.

### Added

- **Схема Postgres** (`schema/schema_v0.sql`): 6 таблиц, 19 функций,
  коды ошибок LS001…WK002.
  - `tasks`, `artifacts`, `task_deps`, `workers`, `events`, `reviews`.
  - Guarded FSM с 6 состояниями.
  - Fencing token через `lease_epoch`.
  - Хэш-цепочка событий (`prev_hash`, `hash`, `verify_chain`).
  - Recursive cascade в `reopen_task`.
  - Cycle check в `add_dep`.
- **Rust-workspace** (`crates/`):
  - `swarm-core` — библиотека: `SwarmDispatcher`, ошибки, типы.
  - `swarm-watchdog` — цикл `reclaim_expired` + `promote_ready`.
  - `swarm-worker` — воркер-заглушка (без LLM).
  - `swarm-cli` — `seed`, `status`, `review`, `reopen`, `unblock`.
- **Тесты**:
  - 26 SQL-тестов на Postgres 16 (`tests/test_v0.py`).
  - 8 unit-тестов Rust.
  - 5 integration-тестов Rust на живом Postgres.
- **Docker Compose**: postgres + watchdog + 2 воркера + cli.
- **Native-развёртывание**: без Docker и без root (Postgres 18.6 в
  user-space).
- **Документация** (`docs/`):
  - `PROJECT_STATE.md`, `METHODOLOGY.md`, `ARCHITECTURE.md`.
  - `ARCHITECTURE_DECISIONS.md` — 11 ADR.
  - `PLATFORM_COMPARISON.md`, `THREAT_MODEL.md`.
  - `BRIEF_FOR_AGENTS.md`, `PLAIN_EXPLANATION.md`.
  - `ROADMAP.md`, `NEXT_STEPS.md`, `DOC_STRATEGY.md`.
  - `TZ_v0.2_step1_fixes.md` — ТЗ на текущий шаг.

### Fixed (при независимой верификации v0.1)

- `errors.rs`: `Database(#[source] postgres::Error)` →
  `Database(#[from] postgres::Error)`. Без этого `?` не компилировался.
- `worker/main.rs`: E0521 — клон `Lease` перед `thread::spawn`.
  Ссылка не может быть перемещена в поток (`'static`).
- `errors.rs` (тест): `sqlstate_known_codes_map` проверял текст сообщения
  вместо варианта ошибки. Переписан на `matches!(err, DispatcherError::*())`.
- `watchdog/main.rs`: убран неиспользуемый `use tracing::warn`.

### Changed

- `Dockerfile`: `rust:1.80-slim` → `rust:1.98-slim`. Транзитивная
  `block-buffer 0.12` требует `edition2024` (Rust ≥ 1.85).
- `Cargo.toml`: `rust-version = "1.75"` → `"1.85"`. Декларативно неверный
  MSRV исправлен; фактический минимум определяется транзитивными
  зависимостями.
- `docker-compose.yml`: удалён deprecated `version: "3.9"`;
  CLI-сервис без `command` (запуск через `swarm-cli <cmd>`).
- `schema/schema_v0.sql`: права `644` (было `600`) — Postgres в контейнере
  не мог прочитать init-скрипт. Правится через `COPY --chmod=644` в
  Dockerfile.

### Known Limitations

См. `docs/KNOWN_LIMITATIONS.md`. Главное:

- `save_artifact` пишет только metadata в БД, файл на диск не пишется.
- Worker — заглушка (`sleep`), реального LLM нет.
- Planner (LLM-декомпозиция) отсутствует.
- Веб-доска отсутствует (только `cli status`).
- OPA, approvals, мультитенантность — v2.

### Verified by

Независимая верификация OpenClaw на Omarchy + syntog (2026-10-08).
Подробности — `docs/VERIFICATION_LOG.md`.

- `cargo build`: 0 errors, 0 warnings.
- Unit-тесты: 8 passed.
- Integration-тесты: 5 passed.
- Native e2e: OK (Postgres 18.6, user-space).
- Docker Compose e2e: OK (4 контейнера).
- Fencing + reclaim: подтверждено (`{"reclaimed":1}` в логе watchdog).
- `verify_chain=true` по всем задачам.
