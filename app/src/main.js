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

function agentIcons(sessions) {
  const set = new Set(sessions.map((s) => s.agent));
  return [...set].map((a) => (a === 'claude' ? 'C' : 'X')).join(' ');
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
        <span class="name">${p.name}</span>
        <span class="agents">${agentIcons(p.sessions)}</span>
      </div>
      <div class="status-row">
        <span class="dot"></span>
        <span class="status">${STATUS_LABEL[p.status]}</span>
        <span class="time">${relTime(p.last_activity)}</span>
      </div>
      <div class="title">${(top.title || '').replace(/</g, '&lt;')}</div>
      <div class="actions">
        <span class="resume">Resume ▸</span>
        <span class="folder" title="Open folder">📁</span>
      </div>`;

    tile.querySelector('.resume').onclick = (e) => {
      e.stopPropagation();
      if (top.id) {
        invoke('resume_session', {
          cwd: p.path,
          agent: top.agent,
          sessionId: top.id,
          fresh: false,
          terminal: 'iterm',
        });
      }
    };
    tile.querySelector('.folder').onclick = (e) => {
      e.stopPropagation();
      invoke('open_folder', { path: p.path });
    };
    tile.onclick = () => {
      if (top.id) {
        invoke('resume_session', {
          cwd: p.path,
          agent: top.agent,
          sessionId: top.id,
          fresh: false,
          terminal: 'iterm',
        });
      }
    };
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
