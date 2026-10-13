import { invoke } from './ipc.js';
import { normaliseKeyEvent } from './key_capture.js';
import { setCaptureActive } from './capture_state.js';

// Open a modal popup to edit one button binding. `options`:
//   pad       : 'dualsense'|'ns_pro'|'ns2_pro' — whose base table (pad_view.js)
//   id        : number
//   label     : string (button display label)
//   currentEntry : ButtonEntry as returned by `get_config` — flattened JSON:
//                  { label, type: 'key'|'macro'|'unbound', value? }
//   macros    : array of macro names from current config
//   onSaved() : called after a successful save
//   onClosed(): called after the popup is dismissed (save or cancel)
//
// While the popup is in Key-capture mode it calls invoke('set_capture_active',
// { active: true }) so the engine pauses synth and self-keystrokes can't be
// captured by accident. The flag is cleared on Save or Cancel.

export function open(options) {
  const root = document.createElement('div');
  root.className = 'bind-popup-overlay';
  root.innerHTML = `
    <div class="bind-popup">
      <div class="bp-title">${escape(options.label)}</div>
      <div class="bp-sub">id ${options.id}</div>

      <div class="seg" role="tablist">
        <button class="seg-item" data-seg="key">Key</button>
        <button class="seg-item" data-seg="macro">Macro</button>
        <button class="seg-item" data-seg="mouse">Mouse</button>
        <button class="seg-item" data-seg="unbound">Unbound</button>
      </div>

      <div class="bp-editor"></div>

      <div class="bp-error" hidden></div>

      <div class="bp-actions">
        <button class="bp-cancel">Cancel</button>
        <button class="bp-save primary">Save</button>
      </div>
    </div>
  `;
  document.body.appendChild(root);

  // State — currentEntry is the flattened JSON: { label, type, value? }
  const entry = options.currentEntry || {};
  let segment = entry.type || 'unbound';
  let capturedKey = segment === 'key'   ? (entry.value ?? null) : null;
  let chosenMacro = segment === 'macro' ? (entry.value ?? null) : null;
  let chosenMouse = segment === 'mouse' ? (entry.value ?? 'left') : 'left';

  const errorEl  = root.querySelector('.bp-error');
  const editorEl = root.querySelector('.bp-editor');

  function show(msg) {
    errorEl.textContent = msg;
    errorEl.hidden = false;
  }
  function hideError() {
    errorEl.hidden = true;
  }

  function renderSeg() {
    root.querySelectorAll('.seg-item').forEach(b => {
      b.classList.toggle('on', b.dataset.seg === segment);
    });
    renderEditor();
  }

  function renderEditor() {
    hideError();
    editorEl.innerHTML = '';

    if (segment === 'key') {
      const box = document.createElement('div');
      box.className = 'capture-box';
      box.tabIndex = 0;

      const updateDisplay = () => {
        if (capturedKey) {
          box.innerHTML = `Current: <code>${escape(capturedKey)}</code><br><span class="hint">Press another key to overwrite.</span>`;
        } else {
          box.innerHTML = `<strong>Press a key to bind…</strong>`;
        }
      };
      updateDisplay();
      editorEl.appendChild(box);

      // Persistent capture: listener stays attached so the user can keep
      // pressing keys to overwrite the binding without re-clicking the box.
      // Each accepted keypress updates the display in place — we no longer
      // rebuild the editor or blur, so focus is preserved.
      const onKey = ev => {
        const r = normaliseKeyEvent(ev);
        if (r.cancel) {
          // Let Escape bubble up to the popup-root close handler.
          return;
        }
        ev.preventDefault();
        ev.stopPropagation();
        if (r.reject) { show(r.reject); return; }
        hideError();
        capturedKey = r.name;
        updateDisplay();
      };
      box.addEventListener('keydown', onKey);
      box.addEventListener('focus', () => {
        setCaptureActive(true);
      });
      box.addEventListener('blur', () => {
        setCaptureActive(false);
      });
      // Auto-focus so the user doesn't have to click the box first.
      setTimeout(() => box.focus(), 0);
    }

    if (segment === 'macro') {
      if (!options.macros.length) {
        const m = document.createElement('div');
        m.className = 'hint';
        m.textContent = 'No macros defined. Add one in the Macros tab first.';
        editorEl.appendChild(m);
        return;
      }
      const sel = document.createElement('select');
      sel.className = 'macro-select';
      for (const name of options.macros) {
        const opt = document.createElement('option');
        opt.value = name;
        opt.textContent = name;
        if (name === chosenMacro) opt.selected = true;
        sel.appendChild(opt);
      }
      if (!chosenMacro) chosenMacro = options.macros[0];
      sel.addEventListener('change', () => { chosenMacro = sel.value; });
      editorEl.appendChild(sel);
    }

    if (segment === 'mouse') {
      const grid = document.createElement('div');
      grid.className = 'mouse-picker';
      const opts = [
        { value: 'left',       label: 'Left' },
        { value: 'middle',     label: 'Middle' },
        { value: 'right',      label: 'Right' },
        { value: 'wheel-up',   label: 'Wheel ↑' },
        { value: 'wheel-down', label: 'Wheel ↓' },
      ];
      for (const o of opts) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'mouse-opt';
        btn.dataset.value = o.value;
        btn.textContent = o.label;
        if (chosenMouse === o.value) btn.classList.add('on');
        btn.addEventListener('click', () => {
          chosenMouse = o.value;
          grid.querySelectorAll('.mouse-opt').forEach(b =>
            b.classList.toggle('on', b.dataset.value === chosenMouse)
          );
        });
        grid.appendChild(btn);
      }
      editorEl.appendChild(grid);
    }

    if (segment === 'unbound') {
      const m = document.createElement('div');
      m.className = 'hint';
      m.textContent = 'This button will do nothing.';
      editorEl.appendChild(m);
    }
  }

  root.querySelectorAll('.seg-item').forEach(b => {
    b.addEventListener('click', () => {
      segment = b.dataset.seg;
      renderSeg();
    });
  });

  root.querySelector('.bp-cancel').addEventListener('click', () => {
    setCaptureActive(false);
    close();
  });

  root.querySelector('.bp-save').addEventListener('click', async () => {
    hideError();
    // ButtonEntry serialises flattened — see comment on `entry` above.
    let entryOut;
    if (segment === 'key') {
      if (!capturedKey) { show('Press a key to bind, or pick Unbound.'); return; }
      entryOut = { label: options.label, type: 'key', value: capturedKey };
    } else if (segment === 'macro') {
      if (!chosenMacro) { show('Pick a macro, or define one in the Macros tab.'); return; }
      entryOut = { label: options.label, type: 'macro', value: chosenMacro };
    } else if (segment === 'mouse') {
      entryOut = { label: options.label, type: 'mouse', value: chosenMouse };
    } else {
      entryOut = { label: options.label, type: 'unbound' };
    }

    try {
      await invoke('set_binding', {
        pad: options.pad ?? 'dualsense', page: 'base', id: options.id, entry: entryOut,
      });
    } catch (e) {
      show(`Save failed: ${e}`);
      return;
    }

    setCaptureActive(false);
    options.onSaved?.();
    close();
  });

  function close() {
    root.remove();
    options.onClosed?.();
  }

  // Esc dismisses the popup as a whole
  root.addEventListener('keydown', ev => {
    if (ev.key === 'Escape') {
      setCaptureActive(false);
      close();
    }
  });

  renderSeg();
}

function escape(s) { return String(s).replace(/[<>&]/g, c => ({'<': '&lt;', '>': '&gt;', '&': '&amp;'}[c])); }
