# Build with a pinned, locally approved Python 3.11 bookworm image.
ARG BASE_IMAGE=docker.m.daocloud.io/library/python:3.11-slim-bookworm
FROM ${BASE_IMAGE}
# Proxy is a build-only operator value; it is never copied into image ENV.
ARG HTTP_PROXY
ARG HTTPS_PROXY
RUN apt-get update && apt-get install -y --no-install-recommends fontconfig fonts-dejavu-core ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --chmod=0555 tectonic /usr/local/bin/tectonic
COPY --chmod=0444 worker.py /opt/prims-workbench/worker.py
RUN mkdir -p /workspace /cache && chown 10001:10001 /workspace && chmod 0555 /opt/prims-workbench
USER 10001:10001
WORKDIR /workspace
ENTRYPOINT ["python3","-I","/opt/prims-workbench/worker.py"]
