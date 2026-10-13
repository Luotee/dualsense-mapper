// Palette keyboard data, copied from the approved mockup (keymap-board.html):
// rows of [tileName, widthUnits, 'dis'?]; null = gap. LABEL = the text drawn
// on a tile when it differs from its name.

export const U = 25, GAP = 3, ROW = 15 * U - GAP;

export const MAIN = [
  [['Esc', 1], [null, 1], ['F1', 1], ['F2', 1], ['F3', 1], ['F4', 1], [null, .5], ['F5', 1], ['F6', 1], ['F7', 1], ['F8', 1], [null, .5], ['F9', 1], ['F10', 1], ['F11', 1], ['F12', 1]],
  [['`', 1], ['1', 1], ['2', 1], ['3', 1], ['4', 1], ['5', 1], ['6', 1], ['7', 1], ['8', 1], ['9', 1], ['0', 1], ['-', 1], ['=', 1], ['Backspace', 2]],
  [['Tab', 1.5], ['Q', 1], ['W', 1], ['E', 1], ['R', 1], ['T', 1], ['Y', 1], ['U', 1], ['I', 1], ['O', 1], ['P', 1], ['[', 1], [']', 1], ['\\', 1.5]],
  [['Caps', 1.75, 'dis'], ['A', 1], ['S', 1], ['D', 1], ['F', 1], ['G', 1], ['H', 1], ['J', 1], ['K', 1], ['L', 1], [';', 1], ["'", 1], ['Enter', 2.25]],
  [['LShift', 2.25], ['Z', 1], ['X', 1], ['C', 1], ['V', 1], ['B', 1], ['N', 1], ['M', 1], [',', 1], ['.', 1], ['/', 1], ['RShift', 2.75]],
  [['LCtrl', 1.5], ['Win', 1.25], ['LAlt', 1.5], ['Space', 6.5], ['RAlt', 1.5], ['Menu', 1.25, 'dis'], ['RCtrl', 1.5]],
];

export const NAV = [[], [['Ins', 1, 'dis'], ['Home', 1], ['PageUp', 1]], [['Del', 1], ['End', 1], ['PageDown', 1]], [], [[null, 1], ['↑', 1], [null, 1]], [['←', 1], ['↓', 1], ['→', 1]]];

export const LABEL = { LShift: 'Shift', RShift: 'Shift', LCtrl: 'Ctrl', RCtrl: 'Ctrl', LAlt: 'Alt', RAlt: 'Alt', Backspace: 'Back', PageUp: 'PgUp', PageDown: 'PgDn' };

export const MOUSE_TILES = ['左鍵', '右鍵', '中鍵', '滾輪上', '滾輪下'];
