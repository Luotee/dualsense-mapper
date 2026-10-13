import { invoke, listen } from './ipc.js';
import * as mappings  from './mappings.js';
import * as macros    from './macros.js';
import * as settings  from './settings.js';
import * as activity  from './activity.js';
import * as kbdMirror from './keyboard_mirror.js';
import * as padView   from './pad_view.js';

const tabs = ['mappings', 'macros', 'settings'];

function activate(tab) {
  tabs.forEach(t => {
    document.querySelector(`[data-tab="${t}"]`).classList.toggle('active', t === tab);
    document.getElementById(`pane-${t}`).classList.toggle('active', t === tab);
  });
}
tabs.forEach(t => {
  document.querySelector(`[data-tab="${t}"]`).addEventListener('click', () => activate(t));
});

document.getElementById('btn-settings').addEventListener('click', () => activate('settings'));

const statusEl   = document.querySelector('.status');
const statusText = document.getElementById('status-text');
function setStatus(connected, name, transport) {
  if (connected) {
    statusEl.classList.add('connected');
    statusText.textContent = `Connected · ${name}${transport ? ' · ' + transport : ''}`;
  } else {
    statusEl.classList.remove('connected');
    statusText.textContent = 'Waiting for controller…';
  }
}
// Tauri events don't replay — if a pad is already paired before this script
// runs, the engine's initial Connected emit was sent before our listen()
// registered. Seed from a synchronous query first.
try {
  const initial = await invoke('get_controller_status');
  padView.setStatus(initial);
  if (initial) setStatus(true, initial.name, initial.transport);
} catch (e) {
  console.warn('get_controller_status failed', e);
}
await listen('controller-status', s => {
  padView.setStatus(s);
  setStatus(s.connected, s.name, s.transport);
});

await mappings.init();
await macros.init();
await settings.init();
await activity.init();
await kbdMirror.init();
