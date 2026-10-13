-- Migration 007: http_status в cost ledger (Issue #2).
--
-- Проблема: Issue #2 требует записывать HTTP-код каждого вызова LLM, но в
-- `llm_calls` такой колонки не было. Статус кое-как зашивался в текстовое
-- поле `error_code` (например, 'HTTP 401'), а у успешных вызовов HTTP-код не
-- сохранялся вовсе — сверять «успех/ошибка» по коду было нельзя.
--
-- Фикс: явная колонка `http_status int` (nullable: у transport/timeout/parse
-- ошибок HTTP-кода нет — это честный NULL, а не выдуманный 0).
--
-- Заполнение существующих строк: из `error_code` формата 'HTTP <code>'.
-- Успешные вызовы (success = true) — 200.
--
-- Идемпотентно: add column if not exists + update по условию.

alter table public.llm_calls
  add column if not exists http_status int;

comment on column public.llm_calls.http_status is
  'Issue #2: HTTP-код вызова LLM (200 успех; 401/403/429/5xx ошибки; NULL — нет HTTP-ответа: transport/timeout/parse)';

-- Бэкфилл: успешные вызовы до появления колонки отвечали 200.
update public.llm_calls
   set http_status = 200
 where http_status is null
   and success;

-- Бэкфилл: ошибки с error_code вида 'HTTP 401' -> 401.
update public.llm_calls
   set http_status = substring(error_code from 'HTTP ([0-9]{3})')::int
 where http_status is null
   and error_code ~ '^HTTP [0-9]{3}$';

-- Владелец — swarm_owner (инвариант каталога, см. migration 003).
alter table public.llm_calls owner to swarm_owner;

-- Индекс для сверки: разрез по кодам ответа.
create index if not exists llm_calls_http_status_idx
  on public.llm_calls (http_status)
  where http_status is not null;
