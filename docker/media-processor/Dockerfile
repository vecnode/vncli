# Debian 12 slim
FROM debian:12-slim

# Install required dependencies.
# Note: no texlive here — PDF rendering uses tectonic (installed below), a small
# self-contained xetex-compatible engine, which makes the image build fast and
# avoids the huge/flaky texlive package set.
RUN apt-get update -o Acquire::Retries=5 && \
    apt-get install -y --no-install-recommends --fix-missing \
    -o Acquire::Retries=5 \
    git \
    curl \
    jq \
    bash \
    ca-certificates \
    pandoc \
    fontconfig \
    fonts-lmodern \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Install tectonic as the pandoc PDF engine. Use the statically-linked musl
# build so it needs no shared libraries (the dynamic -gnu build wants
# libgraphite2/harfbuzz/fontconfig at runtime).
ARG TECTONIC_VERSION=0.16.9
RUN curl --proto '=https' --tlsv1.2 -fsSL -o /tmp/tectonic.tar.gz \
    "https://github.com/tectonic-typesetting/tectonic/releases/download/tectonic%40${TECTONIC_VERSION}/tectonic-${TECTONIC_VERSION}-x86_64-unknown-linux-musl.tar.gz" && \
    tar -xzf /tmp/tectonic.tar.gz -C /usr/local/bin tectonic && \
    rm -f /tmp/tectonic.tar.gz && \
    chmod +x /usr/local/bin/tectonic && \
    tectonic --version

# Pre-warm the tectonic support bundle at build time so the FIRST PDF render is
# fast and works offline (the TeX packages pandoc needs are baked into the image).
# XDG_CACHE_HOME pins the cache to a fixed path that the non-root runtime user
# (any uid — Linux runs with --user $(id -u)) can also read/extend.
ENV XDG_CACHE_HOME=/opt/cache
RUN mkdir -p /opt/cache && \
    printf '# warmup\n\nHello, *world*.\n' | \
    pandoc -f markdown --pdf-engine=tectonic -o /tmp/warm.pdf - && \
    rm -f /tmp/warm.pdf && \
    chmod -R a+rwX /opt/cache && \
    mkdir -p /outputs && chmod a+rwX /outputs

# Install Python web dependencies
RUN python3 -m pip install --no-cache-dir \
    --break-system-packages \
    fastapi==0.115.6 \
    "uvicorn[standard]==0.32.1" \
    python-multipart==0.0.20 \
    pypdf==5.1.0

# Set working directory
WORKDIR /app

# Copy local repository into image
COPY . /app

# Normalize script line endings and make scripts executable
RUN sed -i 's/\r$//' /app/docker/media-processor/main.sh \
    /app/docker/media-processor/ui_server.py \
    /app/docker/media-processor/api_server.py && \
    cp /app/assets/icons/black-square.svg /app/docker/media-processor/ui/favicon.svg && \
    chmod +x /app/docker/media-processor/main.sh && \
    chmod +x scripts/ubuntu22/*.sh

# Run as a non-root user (defense-in-depth; the run scripts also drop all caps
# and set no-new-privileges). On Linux the run script overrides this with
# --user $(id -u):$(id -g) so PDFs written to the host Desktop stay user-owned.
RUN useradd --create-home --uid 10001 app

# Web access ports (UI + API)
EXPOSE 8085
EXPOSE 8086

USER app

# Run doc-processor web services by default.
ENTRYPOINT ["/bin/bash", "/app/docker/media-processor/main.sh"]
