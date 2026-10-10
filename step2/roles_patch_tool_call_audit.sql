-- roles_patch_tool_call_audit.sql — v0.4 §7: аудит tool calls (shell tools).
--
-- ПОЧЕМУ ПАТЧ, А НЕ ПРАВКА schema_v0.sql:
--   schema_v0.sql неприкосновенен (ТЗ v0.4 §0 «Неприкосновенно»).
--
-- ПОЧЕМУ ФУНКЦИЯ, А НЕ ПРЯМОЙ INSERT В events:
--   step2/roles.sql §6 явно закрывает внутренние функции (append_event, assert_lease,
--   lock_root, validate_contract, event_hash, c_num) — «журнал нельзя подделать снаружи».
--   Прямой insert в events от имени swarm_worker нарушил бы это свойство. Поэтому
--   воркер получает одну контролируемую точку входа: она проверяет, что аренда жива
--   и принадлежит этому воркеру, и только затем пишет событие от имени swarm_owner.
--
-- Применять ПОСЛЕ: schema_v0.sql → step2/roles.sql → step2b/roles_2b.sql.
-- Идемпотентен.

-- 1. Разрешить kind='tool_call' (констрейнт расширяется, старые значения сохранены).
alter table events drop constraint if exists events_kind_check;
alter table events add constraint events_kind_check check (kind in (
  'created', 'dep_added', 'promoted', 'claimed', 'artifact_saved', 'completed',
  'review_submitted', 'reclaimed', 'reopened', 'cascade_blocked', 'unblocked',
  'tool_call'));

-- 2. Точка входа аудита: только для живого владельца аренды.
create or replace function record_tool_call(
  p_task uuid, p_worker text, p_epoch bigint, p_event_id text, p_payload jsonb)
returns boolean language plpgsql as $$
begin
  -- FIND-04: session_user обязан соответствовать p_worker (иначе WK003).
  -- Без этой строки воркер w1 мог записать событие от имени w2 на его аренде.
  perform assert_worker_identity(p_worker);
  -- Аренда жива и принадлежит этому воркеру? Иначе GR-ошибка.
  perform assert_lease(p_task, p_worker, p_epoch);
  return append_event(p_task, p_event_id, 'tool_call', p_worker, p_payload);
end $$;

-- 3. Права: владелец-не-суперпользователь, исполнение от его имени, доступ только воркеру.
alter function record_tool_call(uuid, text, bigint, text, jsonb) owner to swarm_owner;
alter function record_tool_call(uuid, text, bigint, text, jsonb)
  security definer set search_path = public, pg_temp;
revoke all on function record_tool_call(uuid, text, bigint, text, jsonb) from public;
grant execute on function record_tool_call(uuid, text, bigint, text, jsonb) to swarm_worker;
