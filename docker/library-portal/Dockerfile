# Lightweight viewer/manager for the vncli library/ folder.
# Only app.py is copied in (the build context is this folder), so no PDFs ever
# enter the image. The library is bind-mounted at runtime. PyMuPDF is included
# to render first-page thumbnails for the grid view (cached under
# library/.portal/thumbs/). Debian slim is used for reliable PyMuPDF wheels.
FROM python:3.12-slim

RUN pip install --no-cache-dir "pymupdf>=1.24,<2"

# Run as a non-root user (defense-in-depth; the run scripts also drop all caps
# and set no-new-privileges). On Linux the run script overrides this with
# --user $(id -u):$(id -g) so files under library/.portal stay owned by the user.
RUN useradd --create-home --uid 10001 app

WORKDIR /app
COPY app.py /app/app.py

ENV LIBRARY_DIR=/library \
    PORT=8090

EXPOSE 8090

USER app

CMD ["python", "app.py"]
