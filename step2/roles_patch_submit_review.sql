-- Patch: strict review-gate verification in submit_review (P0 #5).
--
-- Problem (audit v0.6.0-mvp-rc1): the `approved` branch compared review
-- checks by *length only*:
--
--   if jsonb_array_length(p_checks) < jsonb_array_length(t.contract->'checks')
--      or exists (... e->>'status' is distinct from 'pass')
--
-- That lets a reviewer bypass the gates by submitting N fake
-- `{"status":"pass"}` items with no matching `type`, e.g.:
--
--   contract.checks = [{"type":"unit"},{"type":"lint"}]
--   p_checks        = [{"status":"pass"},{"status":"pass"}]   -- must now fail RV003
--
-- Fix: every required check type from `contract.checks` must have at least one
-- item in `p_checks` with the same `type` AND `status = 'pass'`. Extra passing
-- checks are allowed. Missing type, or status != pass, raises RV003.
--
-- Apply AFTER schema/schema_v0.sql and step2/roles.sql (same pattern as
-- roles_patch_planner_status.sql / roles_patch_tool_call_audit.sql).
--
-- NOTE: CREATE OR REPLACE FUNCTION does NOT preserve SECURITY DEFINER /
-- SET search_path, so they are re-declared explicitly below.

create or replace function submit_review(p_task uuid, p_ref text, p_reviewer text,
                                         p_verdict text, p_checks jsonb)
returns text language plpgsql
security definer
set search_path = public, pg_temp
as $$
declare t tasks%rowtype; v_creator text; v_new text;
begin
  select * into t from tasks where id = p_task for update;
  if not found then raise exception 'task_not_found' using errcode = 'GR000'; end if;
  if t.state <> 'review' or t.result_ref is distinct from p_ref then
    raise exception 'review_stale' using errcode = 'RV001';  -- ревью строго привязано к хэшу
  end if;
  select created_by into v_creator from artifacts where task_id = p_task and ref = p_ref;
  if v_creator = p_reviewer then raise exception 'self_review' using errcode = 'RV002'; end if;
  if p_verdict = 'approved' then
    if jsonb_typeof(p_checks) <> 'array' then
      raise exception 'checks_failed' using errcode = 'RV003';
    end if;
    -- Strict gate: каждый required check из contract должен иметь ровно
    -- один pass с тем же type. Только длина больше не принимается.
    if exists (
      select 1
      from jsonb_array_elements(coalesce(t.contract->'checks', '[]'::jsonb)) c
      where not exists (
        select 1
        from jsonb_array_elements(p_checks) p
        where p->>'type' = c->>'type'
          and p->>'status' = 'pass'
      )
    ) then
      raise exception 'checks_failed' using errcode = 'RV003';
    end if;
  end if;
  insert into reviews (task_id, artifact_ref, reviewer, verdict, checks)
  values (p_task, p_ref, p_reviewer, p_verdict, p_checks);

  if p_verdict = 'approved' then v_new := 'done';
  elsif p_verdict = 'changes_requested' then
    v_new := case when t.rework + 1 > c_num(t.contract, '{budget,max_rework}', 3)
                  then 'blocked' else 'ready' end;
  else v_new := 'blocked';
  end if;

  update tasks
     set state = v_new,
         rework = rework + case when p_verdict = 'changes_requested' then 1 else 0 end,
         lease_epoch = lease_epoch + case when v_new = 'ready' then 1 else 0 end,
         block_reason = case when v_new = 'blocked' then
                        case when p_verdict = 'rejected' then 'rejected' else 'max_rework' end end,
         updated_at = now()
   where id = p_task;
  perform append_event(p_task, 'review:' || p_ref || ':' || p_reviewer, 'review_submitted',
                       p_reviewer, jsonb_build_object('verdict', p_verdict, 'new_state', v_new));
  if v_new = 'done' then perform promote_ready(); end if;
  return v_new;
end $$;

alter function submit_review(uuid, text, text, text, jsonb) owner to swarm_owner;
alter function submit_review(uuid, text, text, text, jsonb)
  security definer set search_path = public, pg_temp;

grant execute on function submit_review(uuid, text, text, text, jsonb) to swarm_reviewer;
