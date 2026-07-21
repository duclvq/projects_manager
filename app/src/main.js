const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const grid = document.getElementById('grid');
const search = document.getElementById('search');
const pin = document.getElementById('pin');

let projects = [];
let filter = '';
let pinned = true;

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
  pin.style.opacity = pinned ? '1' : '0.4';
});

listen('snapshot', (e) => {
  projects = e.payload;
  render();
});

invoke('get_snapshot').then((p) => {
  projects = p;
  render();
});
