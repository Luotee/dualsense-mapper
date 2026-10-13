// The key-binding board: one 980×640 screen scaled to the window (spec 3).

import { state, init, onRender } from './board/store.js';
import { render } from './board/view.js';
import { wire } from './board/events.js';
import { resetInk } from './board/tags.js';
import { startLive } from './board/live.js';

const board = document.getElementById('board');

function fit() {
  const k = Math.min(innerWidth / 980, innerHeight / 640);
  board.style.transform = `translate(${(innerWidth - 980 * k) / 2}px, ${(innerHeight - 640 * k) / 2}px) scale(${k})`;
}

function paint() {
  board.className = 'mock' + (mappingOff() ? ' off' : '');
  board.innerHTML = render(state);
}

function mappingOff() {
  const v = state.config?.mapping_enabled?.[state.pad];
  return (v === undefined ? state.pad !== 'ns2_pro' : v) === false;
}

onRender(paint);
await init();
wire(board);
await startLive();
paint();
fit();
addEventListener('resize', fit);
// Tag ink boxes are measured with the pixel font; re-measure once it loaded.
if (document.fonts) document.fonts.ready.then(() => { resetInk(); paint(); });
