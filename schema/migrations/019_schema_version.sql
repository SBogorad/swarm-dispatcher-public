-- 019_schema_version.sql
--
-- Дефект (2026-10-20, следствие разбора §5.3). Контур умел применять миграции,
-- но не умел ОТВЕЧАТЬ, какая схема у него сейчас. Реестр
-- `public.schema_migrations` — единственный источник правды о применённом, но
-- сервисные роли его не видят: таблица принадлежит `swarm_owner`, а
-- `revoke all ... from public` в `05-migrations.sh`/`apply_migrations.sh`
-- закрывает чтение всем, включая `swarm_worker`
-- (`has_table_privilege('swarm_worker','public.schema_migrations','select')`
-- = false — проверено на живом томе 2026-10-20).
--
-- Следствие без этого RPC: код и схема расходятся молча. Ровно так 015–018
-- отсутствовали на живом томе, а воркер при этом работал — без recall и без
-- дедупликации уроков, не падая и не логируя ничего подозрительного. Дефект
-- обнаружился только ручной сверкой каталога.
--
-- Решение: узкий SECURITY DEFINER RPC, отдающий ровно одно значение — имя
-- последней применённой миграции. Он не раскрывает ни контрольные суммы, ни
-- время применения, ни того, кто применял: сервису нужен только факт версии.
--
-- Почему не `grant select on schema_migrations to swarm_worker`: это открыло бы
-- воркеру весь реестр (checksum, applied_by, applied_at) ради одного поля.
-- Least privilege — отдельная функция с единственным возвратом.
--
-- Идемпотентно: create or replace + повторный grant безопасны.
--
-- Почему plpgsql, а не language sql: тело SQL-функции разбирается и проверяется
-- при создании, поэтому `select ... from public.schema_migrations` падал бы с
-- `relation does not exist` на любом пути, где миграции накатываются раньше
-- создания реестра (так делает провижининг acceptance harness — гейт поймал это
-- на первом прогоне). Тело plpgsql проверяется при вызове, а отсутствие таблицы
-- перехватывается явно и означает «схема не инициализирована».

create or replace function public.schema_version()
returns text
language plpgsql
stable
security definer
set search_path = public, pg_temp
as $$
declare
  v text;
begin
  -- Пустой реестр = схема не инициализирована. Возвращаем пустую строку, а не
  -- NULL: вызывающему не нужно различать «нет таблицы» и «нет строк».
  select coalesce(max(filename), '') into v from public.schema_migrations;
  return coalesce(v, '');
exception
  when undefined_table then
    return '';
end;
$$;

-- Владелец и режим — канонический инвариант каталога
-- (step2/test_roles.py::test_catalog_invariants): каждая функция в public
-- обязана быть SECURITY DEFINER с закреплённым search_path и не-суперпользователем
-- во владельцах.
alter function public.schema_version() owner to swarm_owner;
alter function public.schema_version()
  security definer set search_path = public, pg_temp;

-- Права: закрыть по умолчанию, выдать сервисным ролям. Каждая из них обязана
-- уметь проверить версию схемы на старте — иначе гард в коде бесполезен.
revoke all on function public.schema_version() from public;
grant execute on function public.schema_version() to swarm_worker;
grant execute on function public.schema_version() to swarm_watchdog;
grant execute on function public.schema_version() to swarm_planner;
grant execute on function public.schema_version() to swarm_reviewer;
grant execute on function public.schema_version() to swarm_operator;

comment on function public.schema_version() is
  'Версия схемы для сервисов: имя последней применённой миграции (max filename в schema_migrations). Реестр сервисным ролям не читается — только это значение';
