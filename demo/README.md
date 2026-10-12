# Демо-стенд «ЦГЦУ Ассистент» (Powered by Swarm Engine)

Кабина для показа руководству: загрузить суточный Excel-отчёт, задать вопрос
на русском и получить управленческую справку — на экране, без терминала и
таблиц PostgreSQL.

## Запуск одной командой

```bash
./scripts/start_demo.sh            # поднять и открыть браузер
./scripts/start_demo.sh --no-open  # без браузера
./scripts/start_demo.sh --down     # остановить и удалить (включая БД)
```

Состав поднимается из `docker-compose.demo.yml` (боевой `docker-compose.yml`
не затрагивается: свои volume и порты):

| сервис | роль |
|---|---|
| `postgres` | схема v0 (единый источник истины) |
| `demo-bridge` | FastAPI: приём Excel, статус, справка (порт 8080) |
| `demo-worker` | анализ отчёта + формирование справки |
| `demo-reviewer` | строгий review-gate (F2) |
| `demo-scheduler` | `promote_ready` / `reclaim_expired` (сторож) |
| `demo-ui` | Streamlit-кабина (порт 8501) |
| `demo-init` | генерация демо-отчёта в общий volume |

## Что происходит при демонстрации

1. Пользователь перетаскивает Excel в UI и выбирает пресет
   («Справка по задержкам», «Наличие персонала на сменах»,
   «Сводка по вагонообороту»).
2. Мост кладёт файл в `/artifacts/uploads`, ставит задачу публичной функцией
   `add_task`.
3. Сторож переводит `pending → ready`; воркер берёт задачу по `claim_task`
   (fencing-аренда), анализирует отчёт **локально** (pandas) и формирует справку.
4. Ревьюер проводит задачу через строгий review-gate; задача → `done`.
5. UI рисует жизненный цикл (`pending → ready → leased → review → done`) из
   `tasks`/`events` и показывает готовую справку.

## Честная архитектура: что делает движок, а что демо-слой

Это важно проговорить на встрече, чтобы не выдать желаемое за действительное.

**Работает по-настоящему (ядро).** Мост и демо-воркер говорят с движком ровно
теми же публичными SQL-функциями, что и Rust-воркер: `register_worker`,
`add_task`, `claim_task`, `heartbeat`, `save_artifact`, `complete_task`,
`submit_review`, `fail_task`. Поэтому в демо реальны: fencing по `lease_epoch`,
хэш-цепочка событий, запрет self-review (RV002), строгая проверка набора
`checks` по ключам контракта (RV003, вариант A), реестр типов проверок
(`cmd|test|unit|lint|manual`). Всё это проверено E2E-прогоном.

**Демо-слой (осознанное упрощение).**
- Воркер демо — Python-процесс, а не Rust `swarm-worker`. Причина: в ядре
  v0.5.3 `Sandbox::start`/`agent_loop::run` к рабочему циклу воркера **не
  подключены** (см. `docs/KNOWN_LIMITATIONS.md` §5.6/§5.11), а в образе
  `agent-sandbox` нет Python. Прятать это не нужно: доклад честнее, если
  сказать «аналитику исполняет сервис контура», а не «рой сам крутит Docker».
- Аналитика Excel выполняется в демо-слове (pandas). Рой обеспечивает
  постановку задачи, аренды, аудит и журнал.
- Сторож — отдельный Python-цикл, дергающий те же две функции, что и Rust
  `swarm-watchdog`.

**Текст справки: два режима.**
- `DEMO_PREFER_LLM=1` (по умолчанию) — локальная Ollama
  (`DEMO_OLLAMA_URL`, по умолчанию `http://host.docker.internal:11434`,
  модель `DEMO_OLLAMA_MODEL`, по умолчанию `deepseek-coder:6.7b`).
  Если контейнер не видит Ollama — автоматический откат.
- `DEMO_PREFER_LLM=0` или недоступная модель — детерминированный rule-based
  шаблон. Гарантирует демонстрацию без сети и без модели.

В обоих режимах **цифры берутся из Excel** (локальный анализ); LLM только
формулирует текст. Можно честно сказать: «данные не покидали контур».

## Переменные окружения

| переменная | смысл | по умолчанию |
|---|---|---|
| `DEMO_DB_PASSWORD` | пароль БД демо | `swarm` |
| `DEMO_UI_PORT` / `DEMO_API_PORT` | порты кабины / моста | `8501` / `8080` |
| `DEMO_PREFER_LLM` | `1` — спросить Ollama, `0` — только шаблон | `1` |
| `DEMO_OLLAMA_URL` | адрес Ollama | `http://host.docker.internal:11434` |
| `DEMO_OLLAMA_MODEL` | модель Ollama | `deepseek-coder:6.7b` |
| `DEMO_OLLAMA_TIMEOUT` | таймаут запроса к Ollama, с | `90` |
| `DEMO_LEASE_SEC` | аренда задачи, с | `120` |

Моделей Qwen2.5-Coder / Llama-3.1 на стенде нет; доступны
`deepseek-coder:6.7b` и `nomic-embed-text`. При необходимости
`ollama pull qwen2.5-coder:7b` и задать `DEMO_OLLAMA_MODEL`.

## Проверка без UI (быстрый smoke)

```bash
# 1) генерация отчёта
python3 demo/generator/gen_report.py --out demo/samples/

# 2) анализ и справка без БД
python3 demo/lib/analyze.py demo/samples/cgu_daily_report.xlsx | head
python3 demo/lib/summarize.py demo/samples/cgu_daily_report.xlsx | head
```

## Структура

```
demo/
  generator/gen_report.py   # генератор суточного отчёта (детерминированный)
  lib/analyze.py            # локальный анализ Excel (метрики)
  lib/summarize.py          # справка: Ollama или rule-based fallback
  db/client.py              # клиент публичного SQL-протокола движка
  engine/worker.py          # воркер: claim → анализ → save_artifact → complete
  engine/reviewer.py        # ревьюер: строгий review-gate
  engine/scheduler.py       # promote_ready / reclaim_expired
  bridge/api.py             # FastAPI-мост
  ui/app.py                 # Streamlit-кабина
docker-compose.demo.yml
scripts/start_demo.sh
```

## Ограничения демо-стенда

- Пароль БД по умолчанию `swarm` — **только для демо в закрытом контуре**;
  для реального показа задайте `DEMO_DB_PASSWORD`.
- Порт 8080/8501 отдаётся на хост; для показа «из контура» ограничьте доступ
  (bind на нужный интерфейс или обратный прокси).
- Стенд не претендует на роль боевого контура: это витрина над тем же
  движком, с честно разделёнными демо- и ядровыми частями.
