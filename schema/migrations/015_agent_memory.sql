-- Migration 015: долгосрочная память роя — `reflect` + `correct` + `encode-lessons`.
--
-- Дефект (найден при разборе pstack-навыков `reflect`, `correct`,
-- `principle-encode-lessons-in-structure`):
--   У агента в CLI есть `MEMORY.md`: между сессиями он помнит, на чём обжёгся.
--   У роя такой памяти НЕТ. `012_claim_task_rework_feedback` и
--   `013_task_parent_artifacts` живут ВНУТРИ одного графа: как только граф
--   завершился, у роя наступает амнезия, и в следующем графе воркеры
--   повторяют те же ошибки на ревью. Состояние роя живёт только в PostgreSQL,
--   у контейнера воркера нет ни `MEMORY.md`, ни диска (`--read-only`, tmpfs).
--
-- Решение: таблица `public.agent_memory` — общий `MEMORY.md` роя под ACID и
--   RBAC. Два контура:
--
--   Контур 1 (эпизодическая память, `correct`/`reflect`):
--     * триггер `tasks_consolidate_memory` на `AFTER UPDATE OF state`: переход
--       в `done` при `rework > 0` означает «сначала `changes_requested`, потом
--       исправление» — ровно момент, когда урок выучен. Триггер вытаскивает
--       последний отказ ревью и пишет урок;
--     * RPC `task_recall_memory` отдаёт воркеру последние 5 уроков его `kind`
--       из ПРОШЛЫХ графов (исключая собственную задачу), только держателю
--       живой аренды.
--
--   Контур 2 (структурное поумнение, `encode-lessons-in-structure`):
--     * урок, закодированный в структуру (линтер, гейт, тест), делает повторную
--       ошибку невозможной. Живой пример: `fn verify` в `swarm-reviewer`
--       проскочила в PR #5, потому что в гейте не было clippy. Шаг добавлен
--       в `scripts/acceptance_harness.sh` (PR #22) — это и есть закрепление
--       урока в структуре, а не в промпте.
--
-- Почему НЕ `014`: номер `014` уже занят миграцией
--   `014_identity_in_override_migrations.sql` (FIND-NEW-02). Задание просило
--   `014_agent_memory.sql`, но два файла с одним номером применились бы в
--   лексикографическом порядке `014_agent_memory` → `014_identity_...`, и
--   реестр `schema_migrations` получил бы два независимых префикса. Берём 015.
--
-- Извлечение урока: в `reviews.checks` НЕТ поля `detail` (задание исходило из
--   него). Реальная форма элемента — `{id, type, run, status, artifact_ref}`
--   (`crates/swarm-reviewer/src/main.rs::checks_for`). Поэтому урок строится из
--   фактических данных: вердикт + id проваленных проверок + номер реворка.
--   Если провалов нет, урок всё равно фиксируется (факт реворка значим).
--
-- Идемпотентно: `create table if not exists`, `create or replace`,
--   `drop trigger if exists` + повторный `create trigger`, повторные `grant`.

-- ---------------------------------------------------------------------------
-- 1. Таблица уроков.
-- ---------------------------------------------------------------------------
create table if not exists public.agent_memory (
  id           bigserial primary key,
  -- Тип задачи из контракта (`tasks.contract->>'kind'`). `'*'` — урок,
  -- применимый к любому виду работ (например, «всегда заполняй Decision Trail»).
  kind         text not null check (length(btrim(kind)) between 1 and 64),
  source_task_id uuid not null references public.tasks(id) on delete cascade,
  rework_count int not null check (rework_count >= 0),
  lesson       text not null check (length(btrim(lesson)) between 10 and 2000),
  created_at   timestamptz not null default now()
);

-- Шлюз вспоминания читает «последние 5 по kind» — индекс повторяет порядок
-- выборки (`created_at desc, id desc`), иначе на росте таблицы появится сорт.
create index if not exists idx_agent_memory_kind_created
  on public.agent_memory (kind, created_at desc, id desc);

-- Инвариант каталога (`step2/test_roles.py::test_catalog_invariants`): таблицы
-- в public принадлежат swarm_owner, а не суперпользователю.
do $$ begin
  if exists (select 1 from pg_roles where rolname = 'swarm_owner') then
    execute 'alter table public.agent_memory owner to swarm_owner';
    execute 'alter sequence public.agent_memory_id_seq owner to swarm_owner';
  end if;
end $$;

-- Прямой DML закрыт: и запись, и чтение идут через SECURITY DEFINER. Иначе
-- любой воркер писал бы произвольные уроки в общую память роя.
revoke all on table public.agent_memory from public;
revoke all on sequence public.agent_memory_id_seq from public;

-- ---------------------------------------------------------------------------
-- 2. Явная запись урока (`correct`/`reflect` со стороны ревьюера или CLI).
-- ---------------------------------------------------------------------------
create or replace function public.record_agent_lesson(p_task uuid, p_kind text,
                                                      p_lesson text)
returns bigint
language plpgsql security definer set search_path = public, pg_temp as $$
declare v_id bigint;
begin
  -- Задача должна существовать: урок без источника — непроверяемое знание.
  perform 1 from public.tasks where id = p_task;
  if not found then
    raise exception 'task_not_found' using errcode = 'GR000';
  end if;

  insert into public.agent_memory (kind, source_task_id, rework_count, lesson)
  select coalesce(nullif(btrim(p_kind), ''), coalesce(t.contract->>'kind', '*')),
         p_task,
         t.rework,
         btrim(p_lesson)
    from public.tasks t
   where t.id = p_task
  returning id into v_id;

  return v_id;
end $$;

alter function public.record_agent_lesson(uuid, text, text) owner to swarm_owner;
alter function public.record_agent_lesson(uuid, text, text)
  security definer set search_path = public, pg_temp;

revoke all on function public.record_agent_lesson(uuid, text, text) from public;
-- `swarm_cli` входит в `swarm_reviewer` (step2/roles.sql), поэтому явный грант
-- ей не нужен — достаточно членства. Перечислять не будем: лишний грант
-- пришлось бы синхронизировать с ролями вручную.
grant execute on function public.record_agent_lesson(uuid, text, text) to swarm_reviewer;

comment on function public.record_agent_lesson(uuid, text, text) is
  'Pstack reflect/correct: явная запись урока в общую память роя (rework_count берётся из задачи)';

-- ---------------------------------------------------------------------------
-- 3. Автоматический `correct`/`reflect` при преодолении отказа ревью.
-- ---------------------------------------------------------------------------
create or replace function public.consolidate_task_memory() returns trigger
language plpgsql security definer set search_path = public, pg_temp as $$
declare
  v_kind     text;
  v_failed   text;
  v_verdict  text;
  v_lesson   text;
begin
  -- Урок фиксируется только когда ошибка ПРЕОДОЛЕНА: задача дошла до `done`,
  -- но не с первой попытки. `rework > 0` — ровно «был changes_requested».
  if new.state <> 'done' or new.rework <= 0 then
    return new;
  end if;

  v_kind := coalesce(nullif(btrim(new.contract->>'kind'), ''), '*');

  -- Последний отказ ревью по этой задаче: именно он содержит проваленные
  -- проверки. `checks` — jsonb-массив объектов `{id,type,run,status,...}`;
  -- поля `detail` в нём нет (см. шапку миграции).
  select r.verdict,
         (select string_agg(c->>'id', ', ' order by c->>'id')
            from jsonb_array_elements(r.checks) c
           where c->>'status' = 'fail')
    into v_verdict, v_failed
    from public.reviews r
   where r.task_id = new.id
     and r.verdict = 'changes_requested'
   order by r.id desc
   limit 1;

  -- Отказа не было (например, задачу вернули вручную) — писать нечего.
  if v_verdict is null then
    return new;
  end if;

  v_lesson := format(
    'Задача вида %s прошла ревью только с попытки %s. Проваленные проверки: %s. '
    'Корневая ошибка — не выполнены требования контракта, которые проверяет '
    'ревьюер; следующая задача этого вида должна закрыть их до сдачи.',
    v_kind,
    new.rework + 1,
    coalesce(v_failed, 'не указаны'));

  -- Идемпотентность: одна задача — один автоматический урок. Повторный
  -- переход в `done` (reopen → done) не должен плодить дубли.
  insert into public.agent_memory (kind, source_task_id, rework_count, lesson)
  select v_kind, new.id, new.rework, v_lesson
   where not exists (
     select 1 from public.agent_memory m where m.source_task_id = new.id
   );

  return new;
end $$;

alter function public.consolidate_task_memory() owner to swarm_owner;
alter function public.consolidate_task_memory()
  security definer set search_path = public, pg_temp;
revoke all on function public.consolidate_task_memory() from public;

drop trigger if exists tasks_consolidate_memory on public.tasks;
create trigger tasks_consolidate_memory
  after update of state on public.tasks
  for each row execute function public.consolidate_task_memory();

comment on function public.consolidate_task_memory() is
  'Pstack correct/reflect: при переходе задачи в done после rework>0 урок автоматически попадает в agent_memory';

-- ---------------------------------------------------------------------------
-- 4. Шлюз вспоминания: уроки прошлых графов для текущей задачи.
-- ---------------------------------------------------------------------------
create or replace function public.task_recall_memory(p_task uuid, p_worker text,
                                                     p_epoch bigint)
returns table (lesson text, kind text, rework_count int, created_at timestamptz)
language plpgsql security definer set search_path = public, pg_temp as $$
declare v_kind text;
begin
  -- F3: identity воркера (deny-by-default) и живая аренда именно этой задачи.
  -- Без `assert_lease` любой воркер вычитал бы общую память роя по чужой
  -- задаче — это и проверяет мутация S15.
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);

  select coalesce(nullif(btrim(t.contract->>'kind'), ''), '*')
    into v_kind
    from public.tasks t
   where t.id = p_task;

  if v_kind is null then
    raise exception 'task_not_found' using errcode = 'GR000';
  end if;

  return query
    select m.lesson, m.kind, m.rework_count, m.created_at
      from public.agent_memory m
     where m.kind in (v_kind, '*')
       -- Своя задача исключена: её урок ещё не выучен, а черновик не знание.
       and m.source_task_id <> p_task
     order by m.created_at desc, m.id desc
     limit 5;
end $$;

alter function public.task_recall_memory(uuid, text, bigint) owner to swarm_owner;
alter function public.task_recall_memory(uuid, text, bigint)
  security definer set search_path = public, pg_temp;

revoke all on function public.task_recall_memory(uuid, text, bigint) from public;
-- Общая роль `swarm_worker` (nologin) — носитель прав; w1/w2/w_1/gpu_1 в неё
-- входят (step2b/roles_2b.sql), поэтому перечислять их не нужно.
grant execute on function public.task_recall_memory(uuid, text, bigint) to swarm_worker;

comment on function public.task_recall_memory(uuid, text, bigint) is
  'Pstack recall: последние 5 уроков прошлых графов для kind текущей задачи — только держателю живой аренды';
