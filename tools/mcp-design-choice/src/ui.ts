import type { DesignChoiceInput, PreviewHtmlInput } from "./types.js";

export function safeJsonForScript(obj: unknown): string {
  return JSON.stringify(obj)
    .replace(/</g, "\\u003c")
    .replace(/>/g, "\\u003e")
    .replace(/\//g, "\\u002f");
}

export function renderChoicePage(data: DesignChoiceInput): string {
  const optionsJson = safeJsonForScript(data.options);
  const titleEscaped = escapeHtml(data.title);
  const descEscaped = data.description ? `<p class="desc">${escapeHtml(data.description)}</p>` : "";

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${titleEscaped} - Design Choice</title>
  <style>
    :root {
      --bg: #121214;
      --card-bg: #1a1a1e;
      --surface: #242429;
      --border: #323238;
      --text: #ededed;
      --text-dim: #9e9ea7;
      --accent: #3b82f6;
      --accent-hover: #2563eb;
      --accent-fg: #ffffff;
      --success: #10b981;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
    body { background: var(--bg); color: var(--text); min-height: 100vh; display: flex; flex-direction: column; }
    header { background: var(--card-bg); border-bottom: 1px solid var(--border); padding: 12px 20px; display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
    .header-left { min-width: 0; }
    h1 { font-size: 16px; font-weight: 600; color: var(--text); }
    .desc { font-size: 13px; color: var(--text-dim); margin-top: 2px; }
    .controls { display: flex; align-items: center; gap: 8px; }
    .btn-group { display: flex; background: var(--surface); border: 1px solid var(--border); border-radius: 6px; padding: 2px; gap: 2px; }
    .btn-group button { background: transparent; border: none; color: var(--text-dim); padding: 5px 10px; font-size: 12px; border-radius: 4px; cursor: pointer; transition: 0.15s; }
    .btn-group button.active { background: var(--card-bg); color: var(--text); font-weight: 500; }
    .btn-group button:hover:not(.active) { color: var(--text); }
    .tabs-bar { background: var(--surface); border-bottom: 1px solid var(--border); display: flex; gap: 4px; padding: 6px 16px 0 16px; overflow-x: auto; }
    .tab-btn { background: transparent; border: none; border-bottom: 2px solid transparent; color: var(--text-dim); padding: 8px 14px; font-size: 13px; cursor: pointer; transition: 0.15s; border-radius: 6px 6px 0 0; }
    .tab-btn:hover { color: var(--text); background: rgba(255,255,255,0.03); }
    .tab-btn.active { color: var(--text); font-weight: 600; border-bottom-color: var(--accent); background: var(--card-bg); }
    main { flex: 1; display: flex; flex-direction: column; align-items: center; padding: 16px; min-height: 0; }
    .viewport-container { width: 100%; flex: 1; display: flex; justify-content: center; min-height: 400px; transition: width 0.2s ease; }
    .viewport-responsive { width: 100%; max-width: 100%; }
    .viewport-tablet { width: 768px; max-width: 100%; }
    .viewport-mobile { width: 375px; max-width: 100%; }
    .frame-wrapper { width: 100%; background: #ffffff; border-radius: 8px; overflow: hidden; border: 1px solid var(--border); box-shadow: 0 8px 24px rgba(0,0,0,0.4); }
    iframe { width: 100%; height: 100%; border: none; display: block; }
    pre { width: 100%; height: 100%; margin: 0; padding: 16px; background: #18181b; color: #a1a1aa; font-family: monospace; font-size: 13px; overflow: auto; white-space: pre-wrap; }
    footer { background: var(--card-bg); border-top: 1px solid var(--border); padding: 12px 20px; display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
    .footer-left { display: flex; align-items: center; gap: 12px; flex: 1; max-width: 500px; }
    .footer-right { display: flex; align-items: center; gap: 10px; }
    .feedback-input { width: 100%; background: var(--surface); border: 1px solid var(--border); color: var(--text); border-radius: 6px; padding: 8px 12px; font-size: 13px; outline: none; }
    .feedback-input:focus { border-color: var(--accent); }
    .btn-reject { background: transparent; color: #f87171; border: 1px solid rgba(248, 113, 113, 0.35); border-radius: 6px; padding: 8px 14px; font-size: 13px; font-weight: 500; cursor: pointer; transition: 0.15s; white-space: nowrap; }
    .btn-reject:hover { background: rgba(248, 113, 113, 0.12); border-color: #f87171; }
    .btn-choose { background: var(--accent); color: var(--accent-fg); border: none; border-radius: 6px; padding: 8px 18px; font-size: 13px; font-weight: 600; cursor: pointer; transition: 0.15s; display: flex; align-items: center; gap: 6px; white-space: nowrap; }
    .btn-choose:hover { background: var(--accent-hover); }
    .modal-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.75); backdrop-filter: blur(6px); display: none; place-items: center; z-index: 100; padding: 16px; }
    .modal-card { background: var(--card-bg); border: 1px solid var(--border); border-radius: 12px; width: 100%; max-width: 520px; padding: 24px; box-shadow: 0 16px 36px rgba(0, 0, 0, 0.5); }
    .modal-badge { display: inline-block; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px; color: #f87171; background: rgba(248, 113, 113, 0.12); padding: 3px 8px; border-radius: 4px; margin-bottom: 8px; }
    .modal-card h3 { font-size: 17px; font-weight: 600; color: var(--text); margin-bottom: 6px; }
    .modal-card p { font-size: 13px; color: var(--text-dim); line-height: 1.5; margin-bottom: 16px; }
    .reject-textarea { width: 100%; background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 12px; color: var(--text); font-size: 13px; line-height: 1.45; outline: none; resize: vertical; min-height: 100px; box-sizing: border-box; }
    .reject-textarea:focus { border-color: #f87171; }
    .modal-actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 18px; }
    .btn-cancel { background: transparent; border: 1px solid var(--border); color: var(--text-dim); padding: 8px 16px; border-radius: 6px; font-size: 13px; cursor: pointer; transition: 0.15s; }
    .btn-cancel:hover { background: var(--surface); color: var(--text); }
    .btn-request-alt { background: #dc2626; color: #ffffff; border: none; padding: 8px 18px; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer; transition: 0.15s; }
    .btn-request-alt:hover { background: #ef4444; }
    .success-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.85); backdrop-filter: blur(4px); display: none; place-items: center; z-index: 110; text-align: center; }
    .success-card { background: var(--card-bg); border: 1px solid var(--border); padding: 32px; border-radius: 12px; max-width: 440px; margin: 16px; }
    .success-icon { width: 48px; height: 48px; background: rgba(16,185,129,0.15); color: var(--success); border-radius: 50%; display: grid; place-items: center; font-size: 24px; margin: 0 auto 16px auto; }
  </style>
</head>
<body>
  <header>
    <div class="header-left">
      <h1>${titleEscaped}</h1>
      ${descEscaped}
    </div>
    <div class="controls">
      <div class="btn-group" id="viewModeGroup">
        <button type="button" class="active" onclick="setViewMode('rendered')">Preview</button>
        <button type="button" onclick="setViewMode('code')">Source</button>
      </div>
      <div class="btn-group" id="viewportGroup">
        <button type="button" class="active" onclick="setViewport('responsive')">Full</button>
        <button type="button" onclick="setViewport('tablet')">Tablet (768px)</button>
        <button type="button" onclick="setViewport('mobile')">Mobile (375px)</button>
      </div>
    </div>
  </header>

  <nav class="tabs-bar" id="tabsBar"></nav>

  <main>
    <div class="viewport-container viewport-responsive" id="viewportContainer">
      <div class="frame-wrapper">
        <iframe id="previewFrame" sandbox="allow-scripts allow-same-origin"></iframe>
        <pre id="codeBlock" style="display: none;"></pre>
      </div>
    </div>
  </main>

  <footer>
    <div class="footer-left">
      <input type="text" id="feedbackInput" class="feedback-input" placeholder="Feedback for chosen option (optional, e.g. 'looks great, but use smaller font')" />
    </div>
    <div class="footer-right">
      <button type="button" class="btn-reject" id="rejectBtn" onclick="openRejectModal()">
        <span>None of these work (Ask for others)</span>
      </button>
      <button type="button" class="btn-choose" id="chooseBtn" onclick="confirmChoice()">
        <span>Choose this variant</span> &rarr;
      </button>
    </div>
  </footer>

  <div class="modal-overlay" id="rejectModal">
    <div class="modal-card">
      <div class="modal-badge">Unsatisfactory options</div>
      <h3>Ask the agent for alternatives</h3>
      <p>Explain what you dislike about these options or what you want different to guide the next iteration:</p>
      <textarea
        id="rejectFeedbackInput"
        class="reject-textarea"
        placeholder="E.g. The style is too dark, please use warmer colors, horizontal cards, and a more prominent call-to-action..."
        rows="4"
      ></textarea>
      <div class="modal-actions">
        <button type="button" class="btn-cancel" onclick="closeRejectModal()">Cancel</button>
        <button type="button" class="btn-request-alt" id="sendRejectBtn" onclick="confirmRequestAlternatives()">
          Request new variants &rarr;
        </button>
      </div>
    </div>
  </div>

  <div class="success-overlay" id="successOverlay">
    <div class="success-card">
      <div class="success-icon">&#10003;</div>
      <h2 id="successTitle">Choice sent to the agent!</h2>
      <p id="successMsg" style="color: var(--text-dim); margin-top: 8px; font-size: 14px;">The agent received your selection and is continuing its work.</p>
    </div>
  </div>

  <script>
    const options = ${optionsJson};
    let activeIndex = 0;
    let currentViewMode = 'rendered';
    let currentViewport = 'responsive';

    function init() {
      const urlParams = new URLSearchParams(window.location.search);
      const optParam = urlParams.get('option');
      if (optParam) {
        const foundIdx = options.findIndex(o => o.id === optParam || o.label === optParam);
        if (foundIdx >= 0) activeIndex = foundIdx;
      }
      const tabsBar = document.getElementById('tabsBar');
      tabsBar.innerHTML = '';
      options.forEach((opt, idx) => {
        const btn = document.createElement('button');
        btn.className = 'tab-btn' + (idx === activeIndex ? ' active' : '');
        btn.textContent = opt.label || opt.id;
        btn.onclick = () => selectTab(idx);
        tabsBar.appendChild(btn);
      });
      updatePreview();
    }

    function selectTab(idx) {
      activeIndex = idx;
      document.querySelectorAll('.tab-btn').forEach((btn, i) => {
        btn.classList.toggle('active', i === idx);
      });
      updatePreview();
    }

    function setViewMode(mode) {
      currentViewMode = mode;
      document.getElementById('viewModeGroup').querySelectorAll('button').forEach(btn => {
        btn.classList.toggle('active', btn.textContent.toLowerCase().includes(mode === 'rendered' ? 'preview' : 'source'));
      });
      const frame = document.getElementById('previewFrame');
      const code = document.getElementById('codeBlock');
      if (mode === 'code') {
        frame.style.display = 'none';
        code.style.display = 'block';
      } else {
        frame.style.display = 'block';
        code.style.display = 'none';
      }
    }

    function setViewport(vp) {
      currentViewport = vp;
      const container = document.getElementById('viewportContainer');
      container.className = 'viewport-container viewport-' + vp;
      document.getElementById('viewportGroup').querySelectorAll('button').forEach(btn => {
        const txt = btn.textContent.toLowerCase();
        btn.classList.toggle('active', 
          (vp === 'responsive' && txt.includes('full')) ||
          (vp === 'tablet' && txt.includes('tablet')) ||
          (vp === 'mobile' && txt.includes('mobile'))
        );
      });
    }

    function updatePreview() {
      const opt = options[activeIndex];
      const frame = document.getElementById('previewFrame');
      const code = document.getElementById('codeBlock');
      frame.src = '/render/' + encodeURIComponent(opt.id) + '?raw=1';
      code.textContent = opt.html;
      document.getElementById('chooseBtn').querySelector('span').textContent = 'Choose: ' + opt.label;
    }

    async function confirmChoice() {
      const opt = options[activeIndex];
      const feedback = document.getElementById('feedbackInput').value.trim();
      const btn = document.getElementById('chooseBtn');
      btn.disabled = true;
      btn.style.opacity = '0.5';

      try {
        const res = await fetch('/api/choose', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action: 'selected', id: opt.id, label: opt.label, feedback: feedback || undefined })
        });
        if (res.ok) {
          document.getElementById('successTitle').textContent = "Choice confirmed!";
          document.getElementById('successMsg').textContent = "Selection received, continuing work.";
          document.getElementById('successOverlay').style.display = 'grid';
        }
      } catch (err) {
        alert("Error sending choice: " + err.message);
        btn.disabled = false;
        btn.style.opacity = '1';
      }
    }

    function openRejectModal() {
      document.getElementById('rejectModal').style.display = 'grid';
      setTimeout(() => document.getElementById('rejectFeedbackInput').focus(), 50);
    }

    function closeRejectModal() {
      document.getElementById('rejectModal').style.display = 'none';
    }

    async function confirmRequestAlternatives() {
      const feedback = document.getElementById('rejectFeedbackInput').value.trim();
      const sendBtn = document.getElementById('sendRejectBtn');
      sendBtn.disabled = true;
      sendBtn.style.opacity = '0.5';

      try {
        const res = await fetch('/api/choose', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            action: 'request_alternatives',
            id: 'none',
            label: 'No satisfactory variants',
            feedback: feedback || undefined,
          }),
        });
        if (res.ok) {
          closeRejectModal();
          document.getElementById('successTitle').textContent = "Alternative request sent!";
          document.getElementById('successMsg').textContent = "Feedback received, generating new variants.";
          document.getElementById('successOverlay').style.display = 'grid';
        }
      } catch (err) {
        alert("Error sending request: " + err.message);
        sendBtn.disabled = false;
        sendBtn.style.opacity = '1';
      }
    }

    init();
  </script>
</body>
</html>`;
}

function escapeHtml(str: string): string {
  return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

export function renderSinglePreviewPage(title: string, rawHtml: string, description?: string): string {
  const titleEscaped = escapeHtml(title);
  const descEscaped = description ? `<p class="desc">${escapeHtml(description)}</p>` : "";
  const codeEscaped = escapeHtml(rawHtml);

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${titleEscaped} - Preview</title>
  <style>
    :root {
      --bg: #121214;
      --card-bg: #1a1a1e;
      --surface: #242429;
      --border: #323238;
      --text: #ededed;
      --text-dim: #9e9ea7;
      --accent: #3b82f6;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
    body { background: var(--bg); color: var(--text); min-height: 100vh; display: flex; flex-direction: column; }
    header { background: var(--card-bg); border-bottom: 1px solid var(--border); padding: 12px 20px; display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
    .header-left { min-width: 0; }
    h1 { font-size: 16px; font-weight: 600; color: var(--text); }
    .desc { font-size: 13px; color: var(--text-dim); margin-top: 2px; }
    .controls { display: flex; align-items: center; gap: 8px; }
    .btn-group { display: flex; background: var(--surface); border: 1px solid var(--border); border-radius: 6px; padding: 2px; gap: 2px; }
    .btn-group button { background: transparent; border: none; color: var(--text-dim); padding: 5px 10px; font-size: 12px; border-radius: 4px; cursor: pointer; transition: 0.15s; }
    .btn-group button.active { background: var(--card-bg); color: var(--text); font-weight: 500; }
    .btn-group button:hover:not(.active) { color: var(--text); }
    main { flex: 1; display: flex; flex-direction: column; align-items: center; padding: 16px; min-height: 0; }
    .viewport-container { width: 100%; flex: 1; display: flex; justify-content: center; min-height: 400px; transition: width 0.2s ease; }
    .viewport-responsive { width: 100%; max-width: 100%; }
    .viewport-tablet { width: 768px; max-width: 100%; }
    .viewport-mobile { width: 375px; max-width: 100%; }
    .frame-wrapper { width: 100%; background: #ffffff; border-radius: 8px; overflow: hidden; border: 1px solid var(--border); box-shadow: 0 8px 24px rgba(0,0,0,0.4); }
    iframe { width: 100%; height: 100%; border: none; display: block; }
    pre { width: 100%; height: 100%; margin: 0; padding: 16px; background: #18181b; color: #a1a1aa; font-family: monospace; font-size: 13px; overflow: auto; white-space: pre-wrap; }
  </style>
</head>
<body>
  <header>
    <div class="header-left">
      <h1>${titleEscaped}</h1>
      ${descEscaped}
    </div>
    <div class="controls">
      <div class="btn-group" id="viewModeGroup">
        <button type="button" class="active" onclick="setViewMode('rendered')">Preview</button>
        <button type="button" onclick="setViewMode('code')">Source</button>
      </div>
      <div class="btn-group" id="viewportGroup">
        <button type="button" class="active" onclick="setViewport('responsive')">Full</button>
        <button type="button" onclick="setViewport('tablet')">Tablet (768px)</button>
        <button type="button" onclick="setViewport('mobile')">Mobile (375px)</button>
      </div>
    </div>
  </header>

  <main>
    <div class="viewport-container viewport-responsive" id="viewportContainer">
      <div class="frame-wrapper">
        <iframe id="previewFrame" src="/render" sandbox="allow-scripts allow-forms allow-same-origin allow-popups"></iframe>
        <pre id="codeBlock" style="display: none;">${codeEscaped}</pre>
      </div>
    </div>
  </main>

  <script>
    function setViewMode(mode) {
      document.getElementById('viewModeGroup').querySelectorAll('button').forEach(btn => {
        btn.classList.toggle('active', btn.textContent.toLowerCase().includes(mode === 'rendered' ? 'preview' : 'source'));
      });
      const frame = document.getElementById('previewFrame');
      const code = document.getElementById('codeBlock');
      if (mode === 'code') {
        frame.style.display = 'none';
        code.style.display = 'block';
      } else {
        frame.style.display = 'block';
        code.style.display = 'none';
      }
    }

    function setViewport(vp) {
      const container = document.getElementById('viewportContainer');
      container.className = 'viewport-container viewport-' + vp;
      document.getElementById('viewportGroup').querySelectorAll('button').forEach(btn => {
        const txt = btn.textContent.toLowerCase();
        btn.classList.toggle('active',
          (vp === 'responsive' && txt.includes('full')) ||
          (vp === 'tablet' && txt.includes('tablet')) ||
          (vp === 'mobile' && txt.includes('mobile'))
        );
      });
    }
  </script>
</body>
</html>`;
}
