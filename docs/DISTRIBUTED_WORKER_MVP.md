# v0.6 Distributed Worker MVP

## Цель

v0.6 доказывает, что один swarm может выполнять задачи на разных устройствах или виртуальных машинах через общий control plane. Основной сценарий batch-first: задача может выполняться часами, а при отказе воркера должна быть безопасно передана другому исполнителю.

Это не распределённый inference внутри одной модели. Воркеры являются гетерогенными исполнителями контрактов: один может иметь локальную LLM, другой — инструменты сборки, третий — память или специализированные capabilities.

## Reference topology

```text
                         private network / VPN
  planner + CLI  ------------------------------+
                                                |
  watchdog  -----------------------------------+-- PostgreSQL 16
                                                |
  worker A (VM, local LLM) --------------------+
  worker B (remote host, tools) ----------------+
  worker C (weak device, batch) ----------------+
```

PostgreSQL является единственным источником истины для v0.x. Воркеры используют pull-модель и выполняют долгую работу вне транзакции. Состояние задачи защищается lease и монотонным `lease_epoch`.

## Worker contract

При регистрации воркер сообщает capability profile в `workers.capabilities` и affinity tags в `workers.affinity_tags`:

```json
{
  "ram_gb": 16,
  "models": {"coder_class": 2},
  "skills": ["rust", "document-analysis"],
  "execution": ["sandbox"],
  "availability": "batch"
}
```

Контракт задачи может задавать `needs.min_ram_gb`, `needs.coder_class` и `needs.affinity_tags`. `claim_task` выбирает только совместимые workers и предпочитает совпадение affinity. Невозможность подобрать исполнителя не является основанием ослаблять проверку capabilities.

`worker_id` не является аутентификацией. Для каждого удалённого воркера используется отдельная per-worker database role; общая роль `swarm_worker` имеет `NOLOGIN`. Вызовы с чужим worker id и spoofing через `SET ROLE` отклоняются.

## Failure and handoff protocol

1. Worker регистрируется и получает профиль capabilities.
2. Worker claims готовую задачу и получает `lease_epoch`.
3. Работа выполняется вне DB-транзакции; worker периодически отправляет heartbeat.
4. Результат публикуется как artifact с content hash и metadata.
5. `complete_task` принимает результат только при совпадении task id, worker id, lease и `lease_epoch`.
6. При остановке или сетевой потере lease истекает.
7. Watchdog выполняет reclaim, увеличивает fencing epoch и возвращает задачу в `ready` либо переводит её в `blocked` по retry/budget policy.
8. Другой совместимый worker получает новую lease.
9. Поздний результат первого worker отклоняется как stale lease.

Именно fencing, а не скорость обнаружения отказа, является главным инвариантом handoff. Поэтому холодный старт локальной модели допустим в batch-режиме.

## Network modes

### Lab / trusted network

Допустим прямой доступ worker к PostgreSQL через private network или VPN. Нужно ограничить firewall правило до адресов control plane и worker nodes, не публиковать PostgreSQL в общий Интернет и выдавать отдельный DSN каждому worker.

### Production-like remote mode

Следующий этап должен добавить authenticated dispatcher API между worker и PostgreSQL. API станет единственной сетевой точкой входа, а SQL-роли останутся внутренней границей control plane.

Минимальные требования API:

- mTLS или короткоживущая per-worker credential;
- серверная привязка credential к `worker_id`;
- register, claim, heartbeat, artifact metadata, complete и reconnect;
- отказ при попытке действовать от имени другого worker;
- audit event для registration, claim, reclaim, handoff и fencing reject.

Не включать публичный PostgreSQL или передавать superuser credentials на удалённую машину.

## Artifact handoff

Текущий v0.x сохраняет в PostgreSQL metadata и `storage_path`, но не доказывает, что файл доступен с другой машины. Поэтому локальный `ARTIFACTS_DIR` нельзя считать remote handoff.

Для v0.6 нужно выбрать один общий backend:

- S3-compatible object storage с content-addressed key `sha256/<digest>`;
  для найденного профиля Beget используются endpoint `s3.ru1.storage.beget.cloud`,
  region `ru-1` и path-style access; пример переменных без credentials находится
  в `.env.artifacts.example`;
- общий POSIX/NFS mount с каноническими относительными keys;
- другой проверяемый CAS, доступный всем worker/reviewer nodes.

Worker сначала сохраняет байты, затем проверяет digest и metadata, и только после этого вызывает `complete_task`. Reviewer на другой машине обязан повторно проверить наличие, размер и SHA-256. PostgreSQL не следует использовать для крупных payloads.

S3-compatible backend теперь реализован в `swarm-worker` как opt-in режим:
worker загружает локальный content-addressed файл, перечитывает объект и сверяет
SHA-256 до записи metadata в PostgreSQL. Без полного `SWARM_ARTIFACT_S3_*`
профиля сохраняется локальный режим. Реальный production-like acceptance всё
равно требует отдельного dedicated bucket/prefix и прогона с двумя узлами.
Нельзя объявлять production-like remote release на основании одного общего
PostgreSQL или только успешной компиляции клиента.

## Acceptance criteria

v0.6 считается принятым только при воспроизводимом прогоне:

- два worker process или VM используют один disposable PostgreSQL;
- profiles различаются, и incompatible worker не получает задачу;
- planner создаёт DAG с `needs` и affinity;
- worker A claims задачу и получает lease epoch;
- worker A принудительно останавливается;
- watchdog reclaims задачу;
- worker B claims её с новым epoch;
- поздний complete от worker A отклоняется;
- artifact reference и hash доступны worker B через общий backend;
- отсутствующий или изменённый artifact отклоняется reviewer;
- чужой worker id, общая `swarm_worker` role и `SET ROLE` spoofing отклоняются;
- журнал содержит registration, claim, reclaim, handoff и fencing result;
- acceptance attestation фиксирует commit, toolchain, PostgreSQL version, exit codes и hashes логов без credentials или персональных данных.

Пока эти критерии не пройдены на двух узлах, v0.6 является design/operational milestone, а не production-ready distributed release.

## Explicit non-goals

- бесшовное объединение VRAM или tensor-parallel inference;
- real-time latency guarantees;
- автоматическая аттестация TPM;
- тендер с денежной ценой;
- публичный Internet-доступ к PostgreSQL;
- передача персональных или рабочих данных в архитектурные документы.
