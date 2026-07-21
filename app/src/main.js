const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const grid = document.getElementById('grid');
const search = document.getElementById('search');
const pin = document.getElementById('pin');
const usageEl = document.getElementById('usage');

let projects = [];
let filter = '';
let pinned = false;

const STATUS_LABEL = {
  needs_you: 'Needs you',
  working: 'Working',
  resumable: 'Resumable',
  offline: 'Offline',
};

function relTime(iso) {
  const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

function esc(s) {
  return (s || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function fmtTokens(n) {
  if (n >= 1e9) return (n / 1e9).toFixed(1) + 'B';
  if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n / 1e3).toFixed(0) + 'K';
  return String(n || 0);
}

function windowLabel(mins) {
  if (mins >= 10080) return 'weekly';
  if (mins >= 1440) return `${Math.round(mins / 1440)}d`;
  if (mins >= 60) return `${Math.round(mins / 60)}h`;
  return `${mins}m`;
}

function fmtReset(unixSecs) {
  if (!unixSecs) return '';
  const d = new Date(unixSecs * 1000);
  const now = new Date();
  const opts =
    d.toDateString() === now.toDateString()
      ? { hour: 'numeric', minute: '2-digit' }
      : { month: 'short', day: 'numeric' };
  return d.toLocaleString([], opts);
}

function renderUsage(u) {
  if (!u) return;
  const rows = [];

  if (u.codex) {
    const plan = u.codex.plan_type ? `<span class="u-plan">${esc(u.codex.plan_type)}</span>` : '';
    const windows = [u.codex.primary, u.codex.secondary]
      .filter(Boolean)
      .sort((a, b) => a.window_minutes - b.window_minutes);
    const limits = [];
    let codexLabelShown = false;
    const codexLabel = () => {
      if (codexLabelShown) return '';
      codexLabelShown = true;
      return `CODEX ${plan}`;
    };

    if (u.codex.context_remaining_percent != null) {
      const remain = Math.max(0, Math.min(100, u.codex.context_remaining_percent));
      limits.push(`
        <div class="u-limit">
          <div class="u-key">${codexLabel()}</div>
          <div class="gauge"><div class="gauge-fill" style="width:${remain.toFixed(0)}%"></div></div>
          <div class="u-read"><b>${remain.toFixed(0)}%</b> left
            <span class="u-meta">context · ${fmtTokens(u.codex.context_used_tokens)} / ${fmtTokens(u.codex.context_window_tokens)}</span>
          </div>
        </div>`);
    }

    for (const w of windows) {
      const remain = Math.max(0, Math.min(100, 100 - w.used_percent));
      limits.push(`
        <div class="u-limit">
          <div class="u-key">${codexLabel()}</div>
          <div class="gauge"><div class="gauge-fill" style="width:${remain.toFixed(0)}%"></div></div>
          <div class="u-read"><b>${remain.toFixed(0)}%</b> left
            <span class="u-meta">${windowLabel(w.window_minutes)} · resets ${fmtReset(w.resets_at)}</span>
          </div>
        </div>`);
    }
    rows.push(`
      <div class="u-row u-group">${limits.join('')}</div>`);
  }

  if (u.claude) {
    const c = u.claude;
    const limits = [
      ['session', c.session],
      ['weekly', c.weekly],
    ];
    rows.push(`
      <div class="u-row u-group">${limits.map(([label, limit], i) => {
        const remain = Math.max(0, Math.min(100, 100 - limit.used_percent));
        const badge = c.stale ? 'stale' : 'live';
        return `
          <div class="u-limit">
            <div class="u-key">${i === 0 ? `CLAUDE <span class="u-plan muted">${badge}</span>` : ''}</div>
            <div class="gauge"><div class="gauge-fill" style="width:${remain.toFixed(0)}%"></div></div>
            <div class="u-read"><b>${remain.toFixed(0)}%</b> left
              <span class="u-meta">${label} · resets ${esc(limit.reset_label)}</span>
            </div>
          </div>`;
      }).join('')}</div>`);
  } else {
    rows.push(`
      <div class="u-row">
        <div class="u-key">CLAUDE</div>
        <div class="gauge"></div>
        <div class="u-read"><b>—</b><span class="u-meta">usage unavailable</span></div>
      </div>`);
  }

  usageEl.innerHTML = rows.join('');
  usageEl.hidden = false;
}

function agentIcons(sessions) {
  const set = new Set(sessions.map((s) => s.agent));
  return [...set].map((a) => (a === 'claude' ? 'C' : 'X')).join(' ');
}

// A session with a live agent process (working / needs_you) is already open in
// a terminal — resuming its exact id conflicts and the agent exits immediately.
function isLive(session) {
  return session && (session.status === 'working' || session.status === 'needs_you');
}

function launchResume(p) {
  const top = p.sessions && p.sessions[0];
  if (!top || !top.id || p.status === 'offline') return;
  invoke('resume_session', {
    cwd: p.path,
    agent: top.agent,
    sessionId: top.id,
    // If this session is already running, open a fresh one instead of colliding.
    fresh: isLive(top),
    terminal: 'terminal',
  });
}

// Open a specific agent in this project: resume that agent's newest session if
// it exists and isn't already running, otherwise start a fresh one in the folder.
function openIn(p, agentName) {
  if (p.status === 'offline') return;
  const s = (p.sessions || []).find((x) => x.agent === agentName);
  invoke('resume_session', {
    cwd: p.path,
    agent: agentName,
    sessionId: s ? s.id : '',
    fresh: !s || isLive(s),
    terminal: 'terminal',
  });
}

function render() {
  const q = filter.trim().toLowerCase();
  grid.innerHTML = '';
  for (const p of projects) {
    const hay = `${p.name} ${(p.sessions[0]?.title) || ''}`.toLowerCase();
    if (q && !hay.includes(q)) continue;

    const top = p.sessions[0] || {};
    const tile = document.createElement('button');
    tile.className = `tile ${p.status}`;
    tile.disabled = p.status === 'offline';
    tile.innerHTML = `
      <div class="tile-head">
        <span class="name">${esc(p.name)}</span>
        <span class="agents">${agentIcons(p.sessions)}</span>
      </div>
      <div class="status-row">
        <span class="dot"></span>
        <span class="status">${STATUS_LABEL[p.status]}</span>
        <span class="time">${relTime(p.last_activity)}</span>
      </div>
      <div class="title">${esc(top.title)}</div>
      <div class="actions">
        <span class="resume">${isLive(top) ? 'New ▸' : 'Resume ▸'}</span>
        <span class="spacer"></span>
        <button class="agent-btn claude" data-agent="claude" title="Open in Claude">Claude</button>
        <button class="agent-btn codex" data-agent="codex" title="Open in Codex">Codex</button>
        <span class="folder" title="Open folder">📁</span>
      </div>`;

    tile.querySelector('.resume').onclick = (e) => {
      e.stopPropagation();
      launchResume(p);
    };
    tile.querySelectorAll('.agent-btn').forEach((btn) => {
      btn.onclick = (e) => {
        e.stopPropagation();
        openIn(p, btn.dataset.agent);
      };
    });
    tile.querySelector('.folder').onclick = (e) => {
      e.stopPropagation();
      invoke('open_folder', { path: p.path });
    };
    tile.onclick = () => launchResume(p);
    grid.appendChild(tile);
  }
}

search.addEventListener('input', () => {
  filter = search.value;
  render();
});

pin.addEventListener('click', async () => {
  pinned = !pinned;
  await getCurrentWindow().setAlwaysOnTop(pinned);
  pin.classList.toggle('off', !pinned);
});

getCurrentWindow().isAlwaysOnTop().then((value) => {
  pinned = value;
  pin.classList.toggle('off', !pinned);
});

listen('snapshot', (e) => {
  projects = e.payload;
  render();
});

listen('usage', (e) => renderUsage(e.payload));

invoke('get_snapshot').then((p) => {
  projects = p;
  render();
});

invoke('get_usage').then(renderUsage);
