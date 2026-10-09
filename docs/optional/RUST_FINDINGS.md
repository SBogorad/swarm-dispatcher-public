# Rust-обёртка: что проверено запуском

Код из присланного сообщения собран и прогнан на Postgres 16 (Rust 1.75 из Ubuntu, версии зависимостей закреплены вручную из-за edition2024).

## Как было прислано
`cargo build` не проходит:
1. Нет `From<postgres::Error>`, а `connect` использует `?`.
2. У `postgres` не включены `with-uuid-1` и `with-serde_json-1`, поэтому `Uuid` и `serde_json::Value` не реализуют `ToSql`/`FromSql`.
3. В workspace перечислены три крейта, которых нет в поставке.

## Минимальные правки (проверены)
- В workspace: `postgres = { version = "0.19", features = ["with-uuid-1", "with-serde_json-1"] }`.
- В `connect`: `.map_err(DispatcherError::from_pg)?` вместо `?`.
  **Не** добавлять `#[from]` к `Database`: любой будущий `?` обойдёт отображение SQLSTATE, и `LeaseLost` превратится в `Database`.

## Баг, которого нет в списке правок step 1
`claim` десериализует контракт в `TaskContract` **после** того, как `claim_task` уже выдал аренду.
SQL принимает контракт без `kind` или с дробным `budget.attempts`, а Rust нет.
Результат: `claim` возвращает `Err(Serialization)`, задача остаётся `leased` до конца аренды (по умолчанию 300 с), затем уходит в reclaim и сжигает попытку.
Проверено тестом `contract_accepted_by_sql_but_not_by_rust_must_not_strand_the_task`: падал до правки, проходит после.

Правка (в `dispatcher.rs` и `types.rs` из этой папки): `Lease.contract` хранит сырой `serde_json::Value`, типизированный разбор вынесен в `Lease::typed_contract()` и не влияет на аренду.

## Тесты
`integration_verified.rs`: 5 тестов (полный цикл, отображение SQLSTATE, зомби и fencing, параллельный claim 8 воркеров на 24 задачах, регрессия выше). Запуск: `cargo test -p swarm-core -- --test-threads=1` (тесты чистят таблицы).

## with_transaction (раздел 5.1 ТЗ step 1 v2)
Код из ТЗ **не компилируется**: `Transaction<'a> { tx: &'a mut postgres::Transaction<'a> }` делает заём бесконечным, ошибки E0597 и E0505 на `tx.commit()`.
Рабочий вариант в `dispatcher.rs`: два лайфтайма `DispatcherTx<'a, 'b>`, `f(&mut w)?` внутри блока, `commit` после. Тип назван `DispatcherTx`, чтобы не путаться с `postgres::Transaction`.
Тесты: откат при локальной ошибке, откат при SQL-ошибке посреди графа, атомарный коммит графа. Проверено мутацией (commit даже при ошибке): тест с локальной ошибкой падает, тест с SQL-ошибкой посреди графа не падает, потому что Postgres сам откатывает транзакцию после SQL-ошибки. Ловит поломку именно первый тест, второй проверяет другое: что при SQL-ошибке не остаётся частичного графа.
