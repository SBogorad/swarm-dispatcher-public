-- 018_recall_memory_dedup.sql
--
-- Дефект (слепое пятно PR #45). `memory_section` в
-- `crates/swarm-worker/src/prompt.rs` дедуплицирует уроки по смысловому
-- отпечатку — но ПОСЛЕ выборки. Сама выборка живёт в SQL:
--
--     order by m.created_at desc, m.id desc
--     limit 5;
--
-- Если пять свежих уроков описывают одну и ту же причину отказа (разные
-- задачи, разные формулировки), SQL отдаёт все пять, Rust схлопывает их в
-- один, и воркер получает 1 урок вместо 5. Четыре уникальных урока, лежащих
-- глубже в таблице, до воркера не доходят никогда.
--
-- Почему это НЕ побайтовые дубли: в 016 стоит
-- `uq_agent_memory_task_outcome (source_task_id, outcome)`, поэтому две
-- строки одной задачи с одним исходом невозможны. Дубли возникают по смыслу:
-- разные задачи приносят одну причину разными словами. Отсюда группировка по
-- отпечатку, а не по равенству строк.
--
-- Что делает миграция:
--   * создаёт `public.agent_memory_fingerprint(text)` — SQL-двойник
--     `lesson_fingerprint` из `prompt.rs:314-328`: нижний регистр, схлопнутые
--     пробелы, хвост после `причина:` обрезан до 80 символов;
--   * пересоздаёт `task_recall_memory` так, что дедупликация идёт ДО `limit 5`:
--     окно 25 самых свежих строк → `distinct on (отпечаток)` → 5 групп;
--   * дословно воспроизводит `assert_worker_identity` и `assert_lease`
--     (`create or replace` тихо снимает защиту, если строку забыть — это уже
--     случалось: 011/012, см. tests/test_migration_identity_preserved.py);
--   * сохраняет `source_task_id <> p_task`, `security definer`,
--     `search_path = public, pg_temp`, владельца `swarm_owner` и грант
--     `swarm_worker` — без гранта функция перестанет быть вызываемой.
--
-- Сигнатура не меняется: `crates/swarm-core/src/dispatcher.rs:244` и
-- `crates/swarm-worker/src/worker.rs:376` читают ровно четыре колонки по
-- именам.
--
-- Идемпотентность: `create or replace` для обеих функций + повторный
-- `revoke`/`grant`. Повторный прогон даёт тот же результат.

-- ---------------------------------------------------------------------------
-- 1. Смысловой отпечаток урока.
--
-- Режим `immutable` обязателен: функция используется в `distinct on`, и от
-- него же зависит планировщик. Все вложенные вызовы (`lower`,
-- `regexp_replace`, `btrim`, `split_part`, `substr`, `strpos`, `left`)
-- immutable в pg_catalog, поэтому объявление честное, а не «на всякий случай».
--
-- Локаль: БД создаётся с `en_US.utf8`, и `lower()` схлопывает кириллицу
-- корректно (проверено: `lower('ВИТОК Patch') = lower('виток patch')` → true).
-- Отдельный `collate "C"` не нужен и навредил бы: он перестал бы складывать
-- регистр не-ASCII, и отпечаток разошёлся бы с Rust-реализацией.
--
-- `security definer` + закреплённый `search_path` — не перестраховка, а
-- инвариант каталога: `step2/test_roles.py::test_catalog_invariants` требует
-- этого от КАЖДОЙ функции в `public`. Обычная (invoker) функция в этой схеме
-- уронила бы гейт — проверено на первом прогоне. `immutable` сохраняется:
-- от настроек сессии функция не зависит.
-- ---------------------------------------------------------------------------
create or replace function public.agent_memory_fingerprint(p_lesson text)
returns text
language sql
immutable
security definer
set search_path = public, pg_temp
as $$
  select case
           when v.collapsed like '%причина:%' then
             split_part(v.collapsed, 'причина:', 1)
             || 'причина: '
             || left(btrim(substr(v.collapsed,
                                   strpos(v.collapsed, 'причина:') + 8)), 80)
           else v.collapsed
         end
    from (
      select btrim(regexp_replace(lower(coalesce(p_lesson, '')), '\s+', ' ', 'g'))
               as collapsed
    ) v;
$$;

alter function public.agent_memory_fingerprint(text) owner to swarm_owner;
-- Внутренний helper: вызывается только из `task_recall_memory`, которая
-- исполняется как владелец. Прямой вызов снаружи не нужен и не даётся.
revoke all on function public.agent_memory_fingerprint(text) from public;

comment on function public.agent_memory_fingerprint(text) is
  'Смысловой отпечаток урока: регистр, пробелы и хвост причины после 80 символов не различают уроки';

-- ---------------------------------------------------------------------------
-- 2. Шлюз вспоминания: дедупликация до LIMIT 5.
--
-- Порядок операций принципиален и объясняет вложенность:
--
--   1. `w` — окно 25 САМЫХ СВЕЖИХ строк. Окно нужно, чтобы `distinct on` не
--      проходил всю таблицу: она растёт от каждой задачи. Почему 25: даже если
--      все 25 верхних строк — пять групп по пять смысловых дублей, уникальных
--      групп всё равно пять, то есть `limit 5` выполнится полностью.
--
--   2. `d` — `distinct on (отпечаток)` поверх окна. `distinct on` требует,
--      чтобы первый ключ `order by` совпадал с аргументом, поэтому сортировка
--      здесь идёт по отпечатку, а внутри группы — по свежести. Выживает самый
--      свежий урок группы: это совпадает с `dedup_lessons`, которая оставляет
--      первый элемент, а вход идёт свежими сверху.
--
--   3. Внешний `order by ... limit 5` возвращает свежесть наружу. Без него
--      выдача шла бы по алфавиту отпечатков.
--
-- Если поставить `limit 25` на шаг 2 (как выглядит «проще»), окно отберётся
-- по алфавиту отпечатка, а не по свежести, и самые новые уроки могут не
-- попасть в выборку вовсе.
-- ---------------------------------------------------------------------------
create or replace function public.task_recall_memory(p_task uuid, p_worker text,
                                                     p_epoch bigint)
returns table (lesson text, kind text, rework_count int, created_at timestamptz)
language plpgsql security definer set search_path = public, pg_temp as $$
declare v_kind text;
begin
  -- F3: identity воркера (deny-by-default) и живая аренда именно этой задачи.
  -- Без `assert_lease` любой воркер вычитал бы общую память роя по чужой
  -- задаче — это и проверяет мутация S15. Порядок важен: identity ДО lease,
  -- иначе чужой воркер получит LS001 вместо WK003.
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
    select d.lesson, d.kind, d.rework_count, d.created_at
      from (
        select distinct on (public.agent_memory_fingerprint(w.lesson))
               w.lesson, w.kind, w.rework_count, w.created_at, w.id
          from (
            select m.lesson, m.kind, m.rework_count, m.created_at, m.id
              from public.agent_memory m
             where m.kind in (v_kind, '*')
               -- Своя задача исключена: её урок ещё не выучен, а черновик не знание.
               and m.source_task_id <> p_task
             order by m.created_at desc, m.id desc
             limit 25
          ) w
         order by public.agent_memory_fingerprint(w.lesson),
                  w.created_at desc, w.id desc
      ) d
     order by d.created_at desc, d.id desc
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
  'Pstack recall: до 5 РАЗНЫХ по смыслу уроков прошлых графов для kind текущей задачи — только держателю живой аренды';
