# order_analysis: recomputes every table and figure in report/generated from the raw data in data/.

FROM python:3.12.12-slim-bookworm
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 MPLCONFIGDIR=/tmp/matplotlib
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY order_analysis ./order_analysis
COPY tests ./tests
WORKDIR /work
ENV PYTHONPATH=/app
ENTRYPOINT ["python", "-m", "order_analysis"]
CMD ["--data", "data", "--out", "report/generated"]
