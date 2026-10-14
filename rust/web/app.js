// The key-binding board: one 980×640 screen scaled to the window (spec 3).

import { state, init, onRender } from './board/store.js';
import { render, outputMode } from './board/view.js';
import { wire } from './board/events.js';
import { resetInk } from './board/tags.js';
import { startLive } from './board/live.js';
import { startMacros } from './board/macros.js';
import { startSettings } from './board/settings.js';
import { startCalibration } from './board/calibration.js';
import { startVigem, maybeProbe } from './board/vigem.js';

const board = document.getElementById('board');

function fit() {
  const k = Math.min(innerWidth / 980, innerHeight / 640);
  board.style.transform = `translate(${(innerWidth - 980 * k) / 2}px, ${(innerHeight - 640 * k) / 2}px) scale(${k})`;
}

function paint() {
  // 關 and 手把 both dim the board; it stays editable (spec v3.2.0 §1).
  board.className = 'mock' + (outputMode(state, state.pad) !== 'kbm' ? ' off' : '');
  board.innerHTML = render(state);
  maybeProbe();
}

onRender(paint);
await init();
wire(board);
await startLive();
startMacros(board);
await startSettings(board);
startCalibration();
startVigem();
paint();
fit();
addEventListener('resize', fit);
// Tag ink boxes are measured with the pixel font; re-measure once it loaded.
if (document.fonts) document.fonts.ready.then(() => { resetInk(); paint(); });
