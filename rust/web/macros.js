// macros.js — Macros tab: list + step editor (spec §8)
//
// JSON shape notes (important):
//   ButtonEntry is serde-flattened: JS receives { label, type, value } not { label, binding: {...} }
//     type === 'macro', value === macro_name
//   MacroDef: { "loop": bool, "steps": [...] }  — serde rename "repeat" → "loop"
//     JS must send { loop: ..., steps: ... } through set_macro
//   MacroStep: { key: str, action: "down"|"up", delay_ms: [min, max] }
//   delay_ms constraint: min < max  (config.rs validate enforces strict <)

import { invoke, listen } from './ipc.js';
import { normaliseKeyEvent } from './key_capture.js';
import { setCaptureActive } from './capture_state.js';
import { allSlots } from './pad_view.js';

let config       = null;   // Current snapshot from get_config
let selectedName = null;   // Which macro is in the editor
let working      = null;   // Deep clone of MacroDef being edited (dirty copy)
let dragSrcIdx   = null;   // Step row being dragged (index)

// ─── Public API ──────────────────────────────────────────────────────────────

export async function init() {
  config = await invoke('get_config');
  render();
  listen('config-changed', async () => {
    config = await invoke('get_config');
    // If the selected macro was deleted externally, clear selection
    if (selectedName && !(selectedName in (config.macros || {}))) {
      selectedName = null;
      working      = null;
    }
    render();
  });
}

// ─── Render ───────────────────────────────────────────────────────────────────

function render() {
  const pane = document.getElementById('pane-macros');
  pane.innerHTML = '';
  const host = el('div', 'macros-host');
  host.appendChild(buildLayout());
  pane.appendChild(host);
}

function buildLayout() {
  const layout = el('div', 'macros-layout');
  layout.appendChild(buildList());
  layout.appendChild(buildEditor());
  return layout;
}

// ─── Left column: macro list ──────────────────────────────────────────────────

function buildList() {
  const list = el('div', 'macro-list');

  const header = el('div', 'macro-list-head');
  const title  = el('strong', '');
  title.textContent = 'Macros';
  const btnNew = el('button', 'btn-tiny');
  btnNew.textContent = '+ New';
  btnNew.addEventListener('click', createNewMacro);
  header.appendChild(title);
  header.appendChild(btnNew);
  list.appendChild(header);

  const items = el('div', 'macro-items');
  for (const name of Object.keys(config.macros || {})) {
    const row = el('div', 'macro-row' + (name === selectedName ? ' selected' : ''));
    row.dataset.name = name;

    const bound = isBound(name);
    const dot = el('span', 'bound-dot' + (bound ? '' : ' off'));
    dot.title = bound ? 'Bound to button(s)' : 'Unbound';
    dot.textContent = '●';

    const nameSpan = el('span', 'm-name');
    nameSpan.textContent = name;

    const def      = config.macros[name];
    const meta     = el('span', 'm-meta');
    const stepCount = (def.steps || []).length;
    meta.textContent = `${stepCount} step${stepCount !== 1 ? 's' : ''}${def.loop ? ' · loop' : ''}`;

    row.appendChild(dot);
    row.appendChild(nameSpan);
    row.appendChild(meta);
    row.addEventListener('click', () => selectMacro(name));
    row.addEventListener('contextmenu', e => {
      e.preventDefault();
      openContextMenu(name, e.clientX, e.clientY);
    });
    items.appendChild(row);
  }
  list.appendChild(items);
  return list;
}

// ─── Right column: step editor ────────────────────────────────────────────────

function buildEditor() {
  const editor = el('div', 'macro-editor');

  if (!selectedName || !config.macros?.[selectedName]) {
    const hint = el('div', 'hint');
    hint.style.cssText = 'text-align:center; padding:40px 0;';
    hint.innerHTML = 'Select a macro on the left or click <strong>+ New</strong>.';
    editor.appendChild(hint);
    return editor;
  }

  // Initialise working copy on first visit (or after save/discard)
  if (!working) working = deepClone(config.macros[selectedName]);

  // Name + loop header row
  const hdr = el('div', 'step-row-actions');
  const nameEl = el('span', '');
  nameEl.style.cssText = 'font-weight:600; font-size:13px; color:var(--text-strong);';
  nameEl.textContent   = selectedName;
  hdr.appendChild(nameEl);

  const loopLabel = el('label', '');
  loopLabel.style.cssText = 'display:flex; align-items:center; gap:5px; font-size:12px; cursor:pointer;';
  const loopChk = el('input', '');
  loopChk.type    = 'checkbox';
  loopChk.checked = !!working.loop;
  loopChk.addEventListener('change', () => { working.loop = loopChk.checked; });
  const loopTxt = document.createTextNode('Loop');
  loopLabel.appendChild(loopChk);
  loopLabel.appendChild(loopTxt);
  hdr.appendChild(loopLabel);
  editor.appendChild(hdr);

  // Step table
  const table = el('table', 'step-table');
  const thead = el('thead', '');
  thead.innerHTML = `<tr class="head">
    <th></th><th>#</th><th>Key</th><th>Action</th>
    <th>Min (ms)</th><th>Max (ms)</th><th></th>
  </tr>`;
  table.appendChild(thead);

  const tbody = el('tbody', '');
  (working.steps || []).forEach((step, i) => {
    tbody.appendChild(buildStepRow(step, i));
  });
  table.appendChild(tbody);
  editor.appendChild(table);

  // Error banner (hidden until needed)
  const errBanner = el('div', 'step-error');
  errBanner.id     = 'step-err-banner';
  errBanner.hidden = true;
  editor.appendChild(errBanner);

  // Quick-tap inline form placeholder (inserted when button clicked)
  const qtHolder = el('div', '');
  qtHolder.id = 'qt-holder';
  editor.appendChild(qtHolder);

  // Action buttons
  const actions = el('div', 'step-row-actions');

  const btnStep = el('button', 'btn-tiny');
  btnStep.textContent = '+ Step';
  btnStep.addEventListener('click', () => {
    working.steps = working.steps || [];
    working.steps.push({ key: '', action: 'down', delay_ms: [30, 60] });
    rerenderEditor();
  });

  const btnQt = el('button', 'btn-tiny');
  btnQt.textContent = '+ Quick tap…';
  btnQt.addEventListener('click', () => openQuickTap(qtHolder, btnQt));

  const grow = el('span', 'grow');

  const btnDiscard = el('button', 'btn-tiny');
  btnDiscard.textContent = 'Discard';
  btnDiscard.addEventListener('click', () => {
    working = null;
    render();
  });

  const btnSave = el('button', 'btn-tiny');
  btnSave.id        = 'btn-macro-save';
  btnSave.textContent = 'Save';
  btnSave.style.color = 'white';
  btnSave.style.background = 'var(--accent)';
  btnSave.style.borderColor = 'var(--accent)';
  btnSave.addEventListener('click', saveMacro);

  actions.appendChild(btnStep);
  actions.appendChild(btnQt);
  actions.appendChild(grow);
  actions.appendChild(btnDiscard);
  actions.appendChild(btnSave);
  editor.appendChild(actions);

  return editor;
}

// Build one step <tr>
function buildStepRow(step, i) {
  const row = el('tr', validateStep(step) ? '' : 'invalid');
  row.dataset.idx = i;
  row.draggable   = true;

  // Drag handle
  const tdHandle = el('td', '');
  const handle   = el('span', 'handle');
  handle.textContent  = '☰';
  handle.title        = 'Drag to reorder';
  tdHandle.appendChild(handle);
  row.appendChild(tdHandle);

  // Row number
  const tdNum = el('td', '');
  tdNum.textContent = i + 1;
  row.appendChild(tdNum);

  // Key capture cell
  const tdKey = el('td', '');
  const keyCell = el('span', 'step-key-cell');
  keyCell.textContent = step.key || '(click to set)';
  keyCell.title       = 'Click to capture key';
  keyCell.addEventListener('click', () => activateKeyCapture(keyCell, i));
  tdKey.appendChild(keyCell);
  row.appendChild(tdKey);

  // Action dropdown
  const tdAction = el('td', '');
  const sel = el('select', '');
  sel.style.cssText = 'font-size:11px; padding:2px 4px;';
  for (const a of ['down', 'up']) {
    const opt = document.createElement('option');
    opt.value    = a;
    opt.textContent = a;
    if (step.action === a) opt.selected = true;
    sel.appendChild(opt);
  }
  sel.addEventListener('change', () => {
    working.steps[i].action = sel.value;
    // No re-render needed for action change (no validation impact)
  });
  tdAction.appendChild(sel);
  row.appendChild(tdAction);

  // Min ms
  const tdMin = el('td', '');
  const minIn = el('input', '');
  minIn.type  = 'number';
  minIn.min   = '1';
  minIn.value = step.delay_ms[0];
  minIn.addEventListener('input', () => {
    working.steps[i].delay_ms[0] = Number(minIn.value) | 0;
    refreshRowValidity(row, i);
    updateSaveButton();
  });
  tdMin.appendChild(minIn);
  row.appendChild(tdMin);

  // Max ms
  const tdMax = el('td', '');
  const maxIn = el('input', '');
  maxIn.type  = 'number';
  maxIn.min   = '1';
  maxIn.value = step.delay_ms[1];
  maxIn.addEventListener('input', () => {
    working.steps[i].delay_ms[1] = Number(maxIn.value) | 0;
    refreshRowValidity(row, i);
    updateSaveButton();
  });
  tdMax.appendChild(maxIn);
  row.appendChild(tdMax);

  // Delete row button
  const tdDel = el('td', '');
  const btnDel = el('button', 'btn-tiny');
  btnDel.textContent = '✕';
  btnDel.title       = 'Remove step';
  btnDel.style.color = 'var(--red)';
  btnDel.addEventListener('click', () => {
    working.steps.splice(i, 1);
    rerenderEditor();
  });
  tdDel.appendChild(btnDel);
  row.appendChild(tdDel);

  // Drag-and-drop handlers
  row.addEventListener('dragstart', e => {
    dragSrcIdx = i;
    e.dataTransfer.setData('text/plain', String(i));
    row.style.opacity = '0.5';
  });
  row.addEventListener('dragend', () => {
    row.style.opacity = '';
    dragSrcIdx = null;
  });
  row.addEventListener('dragover', e => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
  });
  row.addEventListener('drop', e => {
    e.preventDefault();
    const fromIdx = Number(e.dataTransfer.getData('text/plain'));
    const toIdx   = i;
    if (fromIdx === toIdx) return;
    const moved = working.steps.splice(fromIdx, 1)[0];
    working.steps.splice(toIdx, 0, moved);
    rerenderEditor();
  });

  return row;
}

// ─── Key capture for step rows ────────────────────────────────────────────────

function activateKeyCapture(keyCell, stepIdx) {
  keyCell.classList.add('capturing');
  keyCell.textContent = 'Press a key…';
  setCaptureActive(true);

  function onKey(ev) {
    const r = normaliseKeyEvent(ev);
    if (r.cancel) { abort(); return; }
    if (r.reject) {
      keyCell.textContent = r.reject;
      return;
    }
    working.steps[stepIdx].key = r.name;
    keyCell.classList.remove('capturing');
    keyCell.textContent = r.name;
    setCaptureActive(false);
    document.removeEventListener('keydown', onKey);
    // Refresh validity
    const row = keyCell.closest('tr');
    if (row) refreshRowValidity(row, stepIdx);
    updateSaveButton();
  }

  function abort() {
    keyCell.classList.remove('capturing');
    keyCell.textContent = working.steps[stepIdx]?.key || '(click to set)';
    setCaptureActive(false);
    document.removeEventListener('keydown', onKey);
  }

  document.addEventListener('keydown', onKey);
}

// ─── Quick tap mini-form ──────────────────────────────────────────────────────

function openQuickTap(holder, triggerBtn) {
  if (holder.children.length > 0) {
    holder.innerHTML = '';
    return;
  }

  const form = el('div', '');
  form.style.cssText = 'background:var(--card); border:1px solid var(--border-light); border-radius:6px; padding:12px 14px; margin-top:10px; display:flex; flex-direction:column; gap:8px; font-size:12px;';

  const title = el('div', '');
  title.style.cssText = 'font-weight:600; color:var(--text-strong); margin-bottom:2px;';
  title.textContent = 'Quick tap — inserts down + up pair';
  form.appendChild(title);

  // Key capture row
  const keyRow = el('div', '');
  keyRow.style.cssText = 'display:flex; align-items:center; gap:8px;';
  const keyLbl = el('label', '');
  keyLbl.textContent = 'Key:';
  keyLbl.style.width = '70px';
  const keyCell = el('span', 'step-key-cell');
  keyCell.textContent = '(click to set)';

  let capturedKey = null;
  keyCell.addEventListener('click', () => {
    keyCell.classList.add('capturing');
    keyCell.textContent = 'Press a key…';
    setCaptureActive(true);

    function onKey(ev) {
      const r = normaliseKeyEvent(ev);
      if (r.cancel) { abort(); return; }
      if (r.reject) { keyCell.textContent = r.reject; return; }
      capturedKey = r.name;
      keyCell.classList.remove('capturing');
      keyCell.textContent = r.name;
      setCaptureActive(false);
      document.removeEventListener('keydown', onKey);
    }
    function abort() {
      keyCell.classList.remove('capturing');
      keyCell.textContent = capturedKey || '(click to set)';
      setCaptureActive(false);
      document.removeEventListener('keydown', onKey);
    }
    document.addEventListener('keydown', onKey);
  });
  keyRow.appendChild(keyLbl);
  keyRow.appendChild(keyCell);
  form.appendChild(keyRow);

  // Hold duration
  const holdRow = el('div', '');
  holdRow.style.cssText = 'display:flex; align-items:center; gap:8px;';
  const holdLbl = el('label', '');
  holdLbl.textContent = 'Hold (ms):';
  holdLbl.style.width = '70px';
  const holdMin = el('input', ''); holdMin.type = 'number'; holdMin.value = '30'; holdMin.min = '1'; holdMin.style.width = '60px';
  const holdSep = document.createTextNode(' – ');
  const holdMax = el('input', ''); holdMax.type = 'number'; holdMax.value = '60'; holdMax.min = '1'; holdMax.style.width = '60px';
  holdRow.appendChild(holdLbl);
  holdRow.appendChild(holdMin);
  holdRow.appendChild(holdSep);
  holdRow.appendChild(holdMax);
  form.appendChild(holdRow);

  // Post-delay
  const delayRow = el('div', '');
  delayRow.style.cssText = 'display:flex; align-items:center; gap:8px;';
  const delayLbl = el('label', '');
  delayLbl.textContent = 'Post (ms):';
  delayLbl.style.width = '70px';
  const delMin = el('input', ''); delMin.type = 'number'; delMin.value = '20'; delMin.min = '1'; delMin.style.width = '60px';
  const delSep = document.createTextNode(' – ');
  const delMax = el('input', ''); delMax.type = 'number'; delMax.value = '40'; delMax.min = '1'; delMax.style.width = '60px';
  delayRow.appendChild(delayLbl);
  delayRow.appendChild(delMin);
  delayRow.appendChild(delSep);
  delayRow.appendChild(delMax);
  form.appendChild(delayRow);

  // Inline error
  const qtErr = el('div', 'step-error');
  qtErr.hidden = true;
  form.appendChild(qtErr);

  // Insert + Cancel buttons
  const btns = el('div', '');
  btns.style.cssText = 'display:flex; gap:8px; justify-content:flex-end;';
  const btnCancel = el('button', 'btn-tiny');
  btnCancel.textContent = 'Cancel';
  btnCancel.addEventListener('click', () => { holder.innerHTML = ''; });

  const btnInsert = el('button', 'btn-tiny');
  btnInsert.textContent = 'Insert';
  btnInsert.style.cssText = 'background:var(--accent); color:white; border-color:var(--accent);';
  btnInsert.addEventListener('click', () => {
    if (!capturedKey) { qtErr.textContent = 'Capture a key first.'; qtErr.hidden = false; return; }
    const hMin = Number(holdMin.value) | 0;
    const hMax = Number(holdMax.value) | 0;
    const dMin = Number(delMin.value)  | 0;
    const dMax = Number(delMax.value)  | 0;
    if (hMin >= hMax) { qtErr.textContent = 'Hold min must be < max.'; qtErr.hidden = false; return; }
    if (dMin >= dMax) { qtErr.textContent = 'Post-delay min must be < max.'; qtErr.hidden = false; return; }
    working.steps = working.steps || [];
    working.steps.push({ key: capturedKey, action: 'down', delay_ms: [hMin, hMax] });
    working.steps.push({ key: capturedKey, action: 'up',   delay_ms: [dMin, dMax] });
    holder.innerHTML = '';
    rerenderEditor();
  });

  btns.appendChild(btnCancel);
  btns.appendChild(btnInsert);
  form.appendChild(btns);
  holder.appendChild(form);
}

// ─── Save ─────────────────────────────────────────────────────────────────────

async function saveMacro() {
  if (!selectedName || !working) return;
  const errors = (working.steps || []).map((s, i) => {
    if (!s.key) return `Step ${i + 1}: key not set.`;
    if (!validateStep(s)) return `Step ${i + 1}: delay_ms min must be < max.`;
    return null;
  }).filter(Boolean);

  const banner = document.getElementById('step-err-banner');
  if (errors.length) {
    if (banner) { banner.textContent = errors[0]; banner.hidden = false; }
    return;
  }
  if (banner) banner.hidden = true;

  try {
    // MacroDef: serde renames "repeat" → "loop" so JS must send { "loop": bool, steps: [...] }
    const def = { loop: working.loop ?? false, steps: working.steps || [] };
    await invoke('set_macro', { name: selectedName, def });
    working = null;   // config-changed will refresh
  } catch (e) {
    if (banner) { banner.textContent = `Save failed: ${e}`; banner.hidden = false; }
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function validateStep(step) {
  // Rust validate enforces strict min < max
  return step.delay_ms[0] < step.delay_ms[1];
}

function refreshRowValidity(row, i) {
  const step = working.steps[i];
  if (!step) return;
  row.className = validateStep(step) ? '' : 'invalid';
}

function updateSaveButton() {
  const btn = document.getElementById('btn-macro-save');
  if (!btn) return;
  const anyInvalid = (working.steps || []).some(s => !validateStep(s) || !s.key);
  btn.disabled = anyInvalid;
  btn.style.opacity = anyInvalid ? '0.45' : '1';
}

function rerenderEditor() {
  // Replace only the editor half of the layout (preserve list selection)
  const layout = document.querySelector('.macros-layout');
  if (!layout) { render(); return; }
  const oldEditor = layout.querySelector('.macro-editor');
  if (oldEditor) oldEditor.replaceWith(buildEditor());
}

// ─── Macro list operations ────────────────────────────────────────────────────

function selectMacro(name) {
  if (selectedName === name) return;
  selectedName = name;
  working      = null;
  render();
}

async function createNewMacro() {
  let i = 1;
  const base = 'new_macro';
  let name = base;
  while (config.macros?.[name]) { name = `${base}_${i++}`; }
  try {
    await invoke('set_macro', { name, def: { loop: false, steps: [] } });
    selectedName = name;
    working      = null;
    // config-changed will trigger re-render
  } catch (e) {
    alert(`Could not create macro: ${e}`);
  }
}

// ─── Context menu ─────────────────────────────────────────────────────────────

let _ctxEl = null;

function closeContextMenu() {
  if (_ctxEl) { _ctxEl.remove(); _ctxEl = null; }
}

function openContextMenu(name, x, y) {
  closeContextMenu();
  const ctx = el('div', 'macro-context');
  ctx.style.left = `${x}px`;
  ctx.style.top  = `${y}px`;

  const items = [
    { label: 'Rename',    action: () => renamePrompt(name)   },
    { label: 'Duplicate', action: () => duplicate(name)      },
    { label: 'Delete',    action: () => deleteWithCheck(name) },
  ];

  for (const { label, action } of items) {
    const item = el('div', 'macro-context-item');
    item.textContent = label;
    item.addEventListener('click', () => { closeContextMenu(); action(); });
    ctx.appendChild(item);
  }

  document.body.appendChild(ctx);
  _ctxEl = ctx;

  // Close on outside click
  const dismiss = e => {
    if (!ctx.contains(e.target)) { closeContextMenu(); document.removeEventListener('mousedown', dismiss); }
  };
  setTimeout(() => document.addEventListener('mousedown', dismiss), 0);
}

async function renamePrompt(name) {
  const newName = window.prompt(`Rename macro "${name}" to:`, name);
  if (!newName || newName === name) return;
  try {
    await invoke('rename_macro', { old: name, new: newName });
    if (selectedName === name) { selectedName = newName; working = null; }
  } catch (e) {
    alert(`Rename failed: ${e}`);
  }
}

async function duplicate(name) {
  const def = config.macros?.[name];
  if (!def) return;
  let i = 1;
  let copy = `${name}_copy`;
  while (config.macros?.[copy]) { copy = `${name}_copy_${i++}`; }
  try {
    await invoke('set_macro', { name: copy, def: deepClone(def) });
  } catch (e) {
    alert(`Duplicate failed: ${e}`);
  }
}

async function deleteWithCheck(name) {
  const refs = listBindings(name);
  if (refs.length) {
    const ok = window.confirm(
      `"${name}" is bound to ${refs.length} button(s):\n` +
      refs.map(r => `  ${r.pad} ${r.id}: ${r.label}`).join('\n') +
      `\n\nUnbind and delete the macro?`
    );
    if (!ok) return;
  }
  // delete_macro unbinds it on every pad and page itself, in one write.
  try {
    await invoke('delete_macro', { name });
    if (selectedName === name) { selectedName = null; working = null; }
  } catch (e) {
    alert(`Delete failed: ${e}`);
  }
}

// ─── Binding cross-reference helpers ─────────────────────────────────────────
// ButtonEntry is serde-flattened: { label, type, value } — no nested .binding

// Every pad and page (pad_view.js allSlots).

function isBound(name) {
  return listBindings(name).length > 0;
}

function listBindings(name) {
  return allSlots(config)
    .filter(({ entry }) => entry.type === 'macro' && entry.value === name)
    .map(({ pad, id, entry }) => ({ pad, id, label: entry.label }));
}

// ─── Utilities ────────────────────────────────────────────────────────────────

function deepClone(x)  { return JSON.parse(JSON.stringify(x)); }
function el(tag, cls)  { const e = document.createElement(tag); if (cls) e.className = cls; return e; }
