# Docker socket proxy — сужение поверхности для shell-tools (Волна 5.2)

## Проблема

Чтобы воркер мог запускать sandbox-контейнеры, ему нужно обращаться к Docker.
Прямое монтирование `/var/run/docker.sock` даёт **эквивалент root на хосте**:
через сокет можно создать привилегированный контейнер (`--privileged`,
`-v /:/host`) и выйти наружу. Runtime-гейт `WORKER_SHELL_TOOLS=1` делает
включение осознанным, но не уменьшает саму власть сокета.

## Решение: socket-proxy с белым списком API

Воркер не получает сокет напрямую. Он ходит по `DOCKER_HOST=tcp://socket-proxy:2375`,
а прокси (например, `tecnativa/docker-socket-proxy` или `wollomatic/socket-proxy`)
пропускает только те эндпоинты Docker API, которые нужны sandbox:
`CONTAINERS`, `EXEC`, `IMAGES` (read), `INFO`/`PING`. Всё остальное — `403`.

Это не отменяет runtime-гейт, а **сужает то, что может сделать воркер даже
с включённым гейтом**: он может запускать/останавливать/exec-ить контейнеры, но
не может создавать привилегированные (запрещён `POST /containers/create` с
`Privileged`? — нет, формально прокси фильтрует эндпоинты, а не тело; поэтому
дополнительно нужен `--userns`/rootless, см. остаточный риск).

## Конфигурация (docker-compose override)

```yaml
# ops/docker-compose.shell-tools.yml — включается явно:
#   docker compose -f docker-compose.yml -f ops/docker-compose.shell-tools.yml up
services:
  socket-proxy:
    image: tecnativa/docker-socket-proxy:0.2
    environment:
      # Белый список: только то, что нужно sandbox.
      CONTAINERS: "1"     # create/start/stop/exec
      EXEC: "1"
      IMAGES: "1"         # inspect image
      INFO: "1"
      PING: "1"
      # Явный DENY по умолчанию (прокси и так deny-by-default, фиксируем намерение):
      POST: "1"
      BUILD: "0"          # сборку образов воркеру не разрешаем
      COMMIT: "0"
      CONFIGS: "0"
      SWARM: "0"
      SECRETS: "0"
      NODES: "0"
      SERVICES: "0"
      VOLUMES: "0"        # том-инспекция не нужна
      NETWORKS: "0"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
    restart: unless-stopped

  worker-1:
    environment:
      WORKER_SHELL_TOOLS: "1"
      DOCKER_HOST: tcp://socket-proxy:2375
    depends_on: [socket-proxy]

  worker-2:
    environment:
      WORKER_SHELL_TOOLS: "1"
      DOCKER_HOST: tcp://socket-proxy:2375
    depends_on: [socket-proxy]
```

## Остаточный риск (честно)

* Прокси фильтрует **эндпоинты**, не **тело** запроса. Пока разрешён
  `POST /containers/create`, скомпрометированный воркер может запросить
  привилегированный контейнер (`Privileged: true`, `-v /:/host`) — если Docker
  daemon не настроен иначе.
* Поэтому прокси — **сужение**, а не полное устранение. Полное решение —
  rootless Docker / Podman / sysbox / отдельный runner-хост с изоляцией,
  где daemon и так не является root на критичном хосте (§5.6 KNOWN_LIMITATIONS).
* Значение `WORKER_SHELL_TOOLS=1` **не** включаем в базовый compose и не в CI:
  только явный override (`ops/docker-compose.shell-tools.yml`) на выделенном
  стенде.

## Статус

- Runtime-гейт `WORKER_SHELL_TOOLS` — **внедрён** (`crates/swarm-worker/src/main.rs`,
  коммит 1051d6a).
- Прокси-конфигурация — **предоставлена**, но **не включена по умолчанию**
  (по требованию ТЗ: включать на чём-либо, кроме лабораторного стенда, нельзя).
- Полное устранение (rootless/runner-хост) — задача v1.0, §5.6.
