const urlInput = document.getElementById('url');
const statusEl = document.getElementById('status');
const buttons = Array.from(document.querySelectorAll('button.kind'));

function setStatus(text, cls) {
  statusEl.textContent = text;
  statusEl.className = 'status' + (cls ? ' ' + cls : '');
}
function setButtonsDisabled(disabled) {
  for (const b of buttons) b.disabled = disabled;
}

async function startDownload(kind) {
  const url = urlInput.value.trim();
  if (!url) { setStatus('Paste a URL first.', 'err'); return; }

  setButtonsDisabled(true);
  const startedAt = performance.now();
  setStatus('Downloading (' + kind.toUpperCase() + ')...');
  const timer = setInterval(() => {
    const elapsed = ((performance.now() - startedAt) / 1000).toFixed(0);
    setStatus('Downloading (' + kind.toUpperCase() + ')... ' + elapsed + 's elapsed');
  }, 1000);

  try {
    const res = await fetch('/api/download', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url, kind }),
    });
    const data = await res.json().catch(() => ({ detail: res.statusText }));
    if (!res.ok || !data.ok) {
      throw new Error(data.detail || res.statusText);
    }
    setStatus('Saved to ' + data.saved_to + ':\n' + data.filename, 'ok');
  } catch (error) {
    setStatus('Error: ' + error.message, 'err');
  } finally {
    clearInterval(timer);
    setButtonsDisabled(false);
  }
}

for (const b of buttons) {
  b.addEventListener('click', () => startDownload(b.dataset.kind));
}
urlInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') startDownload('mp4');
});
