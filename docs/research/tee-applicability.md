# Research: TEE и применимость для swarm-dispatcher

**Issue:** #34 (часть B) · **Дата:** 2026-10-14 · **Статус:** research, не блокирует разработку
**Связано:** #34 часть A (`docs/research/zkml-optimistic-verification.md`), #12, #8

---

## 0. Постановка вопроса

Часть A показала: криптографически доказать **корректность** ответа LLM нельзя.
Часть B отвечает на другой вопрос: что даёт **TEE** (Trusted Execution Environment)
для нашего сценария — и что **не** даёт.

Ключевое различение, которое надо держать в голове:

- **Целостность вычисления** (TEE): код и модель внутри доверенной среды не подменены
  хостом; аттестация подтверждает, что исполняется именно заявленный артефакт.
- **Корректность результата** (не TEE): правильный ли ответ. TEE гарантирует, что
  посчитано то, что посчитано — не что посчитано **верно**.

---

## 1. Варианты TEE

| Технология | Класс | Особенности |
|---|---|---|
| Intel SGX | CPU, per-process enclave | Малый объём enclave-памяти (EPC); подходит для небольших фрагментов |
| Intel TDX | CPU, VM-level | Целые confidential VM; проще разворачивать сервис целиком |
| AMD SEV-SNP | CPU, VM-level | Confidential VM + аттестация; сильный конкурент TDX |
| ARM CCA | CPU, Realm | Realm-модель, развивающийся стек |
| NVIDIA Confidential Computing (H100/B200) | GPU | Защита GPU-памяти и аттестация GPU↔CPU; ключ для confidential LLM inference |
| Confidential Containers (CoCo) | Оркестрация | TEE-backed pods (Kata + attestation); совместимо с Kubernetes |
| Подписи Ed25519 + trust registry | «Лёгкая» аттестация без TEE | Не TEE; доверие к подписи воркера |

Стандартизация аттестации и её моделей угроз — в
[RFC 9334: Remote ATtestation procedureS (RATS) Architecture](https://www.rfc-editor.org/rfc/rfc9334.html)
(это первоисточник, а не блог). Академический разбор механизмов аттестации —
[Attestation Mechanisms for Trusted Execution Environments Demystified](https://arxiv.org/abs/2206.03780)
(arXiv:2206.03780).

---

## 2. Что TEE даёт

1. **Аттестация воркера.** Можно криптографически подтвердить, что на узле исполняется
   именно заявленный код (и, при GPU-CC, что инференс идёт на GPU внутри доверенной среды).
   Это заменяет/дополняет сегодняшнюю доверенность на уровне роли БД
   (`docs/KNOWN_LIMITATIONS.md` §2.1: «Worker не аутентифицирован — доверие на уровне роли БД»).
2. **Конфиденциальность входных данных.** Промпт и данные не видны хосту/гипервизору.
   Актуально, если появятся сценарии с чувствительными данными.
3. **Защита артефактов от подмены хостом.** Целостность результата на выходе TEE.

## 3. Что TEE НЕ даёт

1. **Корректность ответа модели.** Галлюцинация внутри TEE остаётся галлюцинацией.
   TEE доказывает «эта модель так посчитала», а не «это правда».
2. **Сравнимость разных моделей.** Две модели в двух TEE — два валидных аттестата,
   но не сравнение качества.
3. **Защиту от ошибок самого кода воркера** (баги, неверный контракт).

## 4. Стоимость и совместимость с нашей архитектурой

- **Confidential LLM inference измеримо дороже.** [Confidential LLM Inference: Performance
  and Cost Across CPU and GPU TEEs](https://arxiv.org/abs/2509.18886) (arXiv:2509.18886)
  прогоняет полные пайплайны Llama2 (7B/13B/70B) внутри Intel TDX/SGX с AMX и измеряет
  оверхед и стоимость на CPU и GPU TEE. Вывод по CPU TEE: оверхед порядка **менее 10%**
  throughput на части конфигураций (данные, batch size, длина входа) — но это
  **self-hosted** инференс.
- **Наши модели — через API.** Воркеры вызывают `openai-compatible` провайдера
  (`crates/swarm-llm/src/lib.rs:225`). TEE на нашей стороне защищает **клиент**, но не
  гарантирует ничего о вычислении на стороне relay.fast — у нас нет контроля над их средой.
- **GPU-класс TEE.** NVIDIA CC (H100/B200) — это про self-hosted inference на дорогом
  железе, что противоречит нашей ниши «работает даже на слабом железе»
  (`docs/research/POSITIONING_2026.md`).
- **Confidential Containers (CoCo).** Совместимы с контейнерным деплоем (у нас Docker
  compose), но требуют TEE-capable хоста и усложняют стек — это прямо противоречит
  Принципу 8 методологии («отказ от модных решений без необходимости»).

## 5. Вариант «лёгкой» аттестации без TEE

В audited-документе v6–v10 были подписи Ed25519 у воркеров + trust registry, сознательно
отложенные в v0 (`docs/PROJECT_STATE.md`, раздел «Что НЕ в v0»; `docs/KNOWN_LIMITATIONS.md`
§2.4). Это даёт **идентификацию и non-repudiation** воркера (кто именно прислал результат),
но **не** целостность вычисления и **не** конфиденциальность. Для нашего сценария
(доверенный кластер, слабое железо) это ближе к реальности, чем TEE.

---

## 6. Рекомендация (часть B)

| Направление | Вердикт | Обоснование |
|---|---|---|
| Полный TEE (TDX/SEV-SNP/GPU-CC) для v1.x | **Условная применимость (не для v1.x)** | Несовместимо со слабым гетерогенным железом; модели через API — TEE на нашей стороне не защищает вычисление провайдера; усложняет стек вопреки Принципу 8. **Но:** применимо в конфиденциальном/threat-моделированном контуре (self-hosted inference на TEE-capable железе) — держать как отдельный трек, не закрывать |
| Confidential Containers (CoCo) | **Отложить** | Совместимо с контейнерами, но требует TEE-хостов; пересмотреть при переходе на self-hosted inference |
| Ed25519-подписи воркеров + trust registry | **Внедрять** (при переходе к compliance-профилю) | Даёт non-repudiation идентификации без TEE; было в v6–v10, дешевле и совместимо с текущим стеком |
| TEE как «доказательство качества» | **Отвергнуть** | TEE не гарантирует корректность результата — фундаментальное ограничение |

**Итог:** для текущего v1.x полный TEE не внедряем (не тот класс железа и не решает
задачу качества), но направление **не закрываем**: применимость — в конфиденциальном/
threat-моделированном контуре (self-hosted inference на TEE-capable железе). Вместо TEE
сейчас — **Ed25519-подписи воркеров + trust registry** как дешёвая аттестация
идентичности, когда появится compliance-требование. TEE пересмотреть при появлении
собственного self-hosted inference.

---

## Источники (первоисточники, проверены HTTP 200 на 2026-10-14)

1. RFC 9334 — Remote ATtestation procedureS (RATS) Architecture — https://www.rfc-editor.org/rfc/rfc9334.html
2. Attestation Mechanisms for Trusted Execution Environments Demystified — https://arxiv.org/abs/2206.03780
3. Confidential LLM Inference: Performance and Cost Across CPU and GPU TEEs — https://arxiv.org/abs/2509.18886
4. Confidential Containers (CNCF) — https://confidentialcontainers.org/
5. NVIDIA Confidential Computing — https://docs.nvidia.com/confidential-computing/
6. Azure Confidential Containers — https://learn.microsoft.com/en-us/azure/confidential-computing/confidential-containers
