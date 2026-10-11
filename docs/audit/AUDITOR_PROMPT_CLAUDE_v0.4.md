# Auditor brief — независимая перепроверка (для Claude)

Ты — независимый аудитор. Тебе дан **самодостаточный архив**
`swarm-dispatcher_audit_<HEAD>.zip` (без доступа к GitHub). Проверь
**утверждения** о закрытии замечаний против **первичных артефактов** в архиве.
Не доверяй заголовкам и summary — открывай файлы.

Итоговая ревизия: HEAD указан в `git_metadata.txt` и в `head=` любого XML-тома.
Если `head=` ≠ HEAD из `git_metadata.txt` — пакет устарел, скажи об этом.

## Что внутри
- `FINDINGS_SD_MAPPING_v0.4.md`, `REAUDIT_SD_CLOSURE_v0.4.md` — карты «замечание → фикс».
- `sql/` — `schema_v0.sql` (единственный источник истины), `roles.sql`,
  `roles_2b.sql`, `roles_patch_tool_call_audit.sql`.
- `diffs/` — патчи по каждому коммиту волны 4.
- `evidence/` — логи доказательств (триады + регрессии).
- `bundles/` — полные тексты файлов с хешами (используй для точечного чтения).
- `environment_versions.txt` — версии PG/Docker/Rust.
- `MANIFEST.sha256` — контрольные суммы всех файлов архива.

## Задачи (по каждой — вердикт: подтверждаю / не подтверждаю / недостаточно данных)

1. **Целостность.** Проверь `sha256sum -c MANIFEST.sha256` и совпадение
   `head=` во всех томах с HEAD.
2. **SD-01 (F3).** В `sql/roles.sql` — `swarm_worker nologin`? В
   `sql/roles_2b.sql` — `assert_worker_identity` пропускает **только** точное
   `session_user = 'swarm_worker_'||normalize_worker_id(p_worker)` или `rolsuper`,
   иначе исключение? Все ли обёртки вызывают проверку?
3. **SD-02 (F2).** В `sql/schema_v0.sql` (`submit_review`) — ключ
   `coalesce(id, type||'|'||run)`, требование `count(pass) >= req.n` **по
   каждому ключу**? Повтор одной проверки не закрывает другие?
4. **SD-03 (F4).** `p_tokens < 0` отклоняется? Нет ли overspend-хака? Проверь
   `unblock_task` (требование `p_new_budget_cap > spent_tokens`).
5. **SD-04 (F15).** `jsonb_typeof(...)` вызывается **до** `jsonb_array_length`?
   Невалидные `checks` дают `CT001`, а не ошибку PG?
6. **SD-05 (FIND-04).** В `sql/roles_patch_tool_call_audit.sql` —
   `assert_worker_identity` **до** записи события?
7. **F1** — общий volume `swarm-artifacts`? `volume://` в `worker.rs`? Dockerfile
   runtime = `debian:trixie-slim`?
8. **FIND-01** — `shell-tools` строго за `#[cfg(feature = "shell-tools")]`?
   Нет ли сильной зависимости вне гейта?
9. **FIND-05** — в `sandbox.rs` чтение ограничено (`take(cap)`) до `truncate`?
10. **FIND-08** — в integration-тестах нет `else { return }`; есть `require_dsn()`
    с panic?

## Ключевое правило
**Не засчитывай фикс без первичного evidence-лога.** Для каждой триады
(RED→GREEN→RED) проверь, что:
- RED реально падал на pre-fix коде;
- GREEN проходил после фикса;
- RED после revert снова падал (иначе фикс «вакуумный»).

## Формат ответа
Таблица: `ID | вердикт | доказательство (файл:строка) | замечание`.
Затем — список расхождений и открытых вопросов. Эксплуатационную безопасность
сверх покрытого триадами не подтверждай (`docs/KNOWN_LIMITATIONS.md`).