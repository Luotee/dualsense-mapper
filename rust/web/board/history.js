// 復原／重做 for board binding changes (spec 8): session-only, any pad.
// Entries are whole-pad snapshots; applying one is a set_pad_tables call.

const copy = v => JSON.parse(JSON.stringify(v));

export function createHistory() {
  const undoStack = [], redoStack = [];
  let pending = null;   // the entry the last undo()/redo() handed out, for dropLast
  return {
    get canUndo() { return undoStack.length > 0; },
    get canRedo() { return redoStack.length > 0; },
    record(pad, before) { undoStack.push({ pad, tables: copy(before) }); redoStack.length = 0; pending = null; },
    undo(current) { return step(undoStack, redoStack, current, 'undo'); },
    redo(current) { return step(redoStack, undoStack, current, 'redo'); },
    // The backend refused the entry the last undo/redo returned: forget it,
    // and the opposite-stack entry pushed for it.
    dropLast(dir) {
      if (!pending || pending.dir !== dir) return;
      (dir === 'undo' ? redoStack : undoStack).pop();
      pending = null;
    },
    clear() { undoStack.length = 0; redoStack.length = 0; pending = null; },
  };
  function step(from, to, current, dir) {
    if (!from.length) return null;
    const e = from.pop();
    to.push({ pad: e.pad, tables: copy(current(e.pad)) });
    pending = { dir };
    return { pad: e.pad, tables: copy(e.tables) };
  }
}
