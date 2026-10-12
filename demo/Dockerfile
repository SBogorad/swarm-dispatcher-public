# Демо-контур «Swarm Ops Console»: единый образ для моста, агентов,
# ревьюера, сторожа и консоли. Различаются только командой запуска
# (см. docker-compose.demo.yml).
FROM python:3.12-slim

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=1

WORKDIR /app

COPY demo/requirements.txt /app/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt

COPY demo/ /app/demo/

ENV PYTHONPATH=/app/demo
WORKDIR /app/demo

# По умолчанию — мост (переопределяется в compose).
CMD ["uvicorn", "bridge.api:app", "--host", "0.0.0.0", "--port", "8080"]
