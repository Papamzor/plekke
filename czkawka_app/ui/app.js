import { mwp } from './vendor/mwp-ui/mwp.js';

const tauri = window.__TAURI__;

const TOOLS = {
  'duplicates': { title: 'Duplicates', sub: 'Which files are stored more than once?', grouped: true, none: 'No duplicates found' },
  'big-files': { title: 'Big files', sub: 'Which files take the most space?', grouped: false, none: 'No files found' },
  'empty-folders': { title: 'Empty folders', sub: 'Which folders hold nothing?', grouped: false, none: 'No empty folders found' },
  'empty-files': { title: 'Empty files', sub: 'Which files are 0 bytes?', grouped: false, none: 'No empty files found' },
};
const FOLDERS_KEY = 'czkawka.folders';
const NUMBER = new Intl.NumberFormat(document.documentElement.lang, { maximumFractionDigits: 1 });

const $ = (id) => document.getElementById(id);
const state = { tool: 'duplicates', groups: [], selected: new Set(), scanning: false };
const folders = loadFolders();

// ---- small helpers ----

/** Builds an element; strings become text nodes, so file names can never inject markup. */
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === 'text') node.textContent = value;
    else if (value === true) node.setAttribute(key, '');
    else if (value !== false && value != null) node.setAttribute(key, value);
  }
  node.append(...children);
  return node;
}

function icon(name) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('class', 'mwp-icon');
  svg.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
  use.setAttribute('href', `vendor/mwp-ui/icons.svg#${name}`);
  svg.append(use);
  return svg;
}

const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;

function formatSize(bytes) {
  const units = ['B', 'kB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${NUMBER.format(value)} ${units[unit]}`;
}

function formatDate(seconds) {
  if (!seconds) return '';
  const date = new Date(seconds * 1000);
  const pad = (n) => String(n).padStart(2, '0');
  return `${pad(date.getDate())}/${pad(date.getMonth() + 1)}/${date.getFullYear()} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function splitPath(path) {
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  return { name: path.slice(cut + 1) || path, folder: cut > 0 ? path.slice(0, cut) : '' };
}

// ---- folders ----

function loadFolders() {
  try {
    const saved = JSON.parse(localStorage.getItem(FOLDERS_KEY));
    if (Array.isArray(saved?.included) && Array.isArray(saved?.excluded)) return saved;
  } catch {
    // no storage or bad JSON: start empty
  }
  return { included: [], excluded: [] };
}

function saveFolders() {
  try {
    localStorage.setItem(FOLDERS_KEY, JSON.stringify(folders));
  } catch {
    // remembering folders is a convenience only
  }
}

function renderFolders(kind) {
  const rows = folders[kind].map((path) =>
    el('div', { class: 'mwp-list__row' },
      el('span', { class: 'mwp-list__main' }, el('span', { class: 'mwp-list__title', text: path })),
      el('span', { class: 'mwp-list__trail' },
        el('button', { type: 'button', class: 'mwp-btn mwp-btn--ghost mwp-btn--icon', 'data-size': 's', 'data-remove': path, 'aria-label': `Remove ${path}` }, icon('x')))));
  if (!rows.length) {
    const hint = kind === 'included' ? 'No folders yet' : 'Nothing skipped';
    rows.push(el('div', { class: 'mwp-list__row' }, el('span', { class: 'mwp-list__main' }, el('span', { class: 'mwp-list__meta', text: hint }))));
  }
  $(`${kind}-list`).replaceChildren(...rows);
}

async function addFolders(kind) {
  const picked = await tauri.dialog.open({ directory: true, multiple: true, title: kind === 'included' ? 'Folders to scan' : 'Folders to skip' });
  if (!picked) return;
  for (const path of [picked].flat()) {
    if (!folders[kind].includes(path)) folders[kind].push(path);
  }
  saveFolders();
  renderFolders(kind);
}

function removeFolder(kind, path) {
  folders[kind] = folders[kind].filter((p) => p !== path);
  saveFolders();
  renderFolders(kind);
}

// ---- tools ----

function showTool() {
  const key = location.hash.slice(1);
  state.tool = key in TOOLS ? key : 'duplicates';
  const tool = TOOLS[state.tool];

  for (const link of document.querySelectorAll('.mwp-nav__link')) {
    if (link.getAttribute('href') === `#${state.tool}`) link.setAttribute('aria-current', 'page');
    else link.removeAttribute('aria-current');
  }
  $('tool-title').textContent = tool.title;
  $('tool-sub').textContent = tool.sub;
  document.title = `${tool.title} · Czkawka`;
  for (const block of document.querySelectorAll('[data-options]')) block.hidden = block.dataset.options !== state.tool;
  for (const item of document.querySelectorAll('#select-menu [data-select^="all-but"], #select-menu hr')) {
    item.hidden = !tool.grouped;
    item.disabled = !tool.grouped;
  }
  setResults([], [], false);
}

function scanRequest() {
  const request = { included: folders.included, excluded: folders.excluded };
  if (state.tool === 'duplicates') {
    const minimalFileSize = Math.max(0, Math.round(Number($('dup-min-size').value || 0) * 1000));
    request.tool = { kind: 'duplicates', method: $('dup-method').value, minimalFileSize };
  } else if (state.tool === 'big-files') {
    const count = Math.max(1, Math.trunc(Number($('big-count').value) || 50));
    request.tool = { kind: 'bigFiles', count, smallest: $('big-mode').value === 'smallest' };
  } else if (state.tool === 'empty-folders') {
    request.tool = { kind: 'emptyFolders' };
  } else {
    request.tool = { kind: 'emptyFiles' };
  }
  return request;
}

async function scan() {
  if (!folders.included.length) {
    mwp.toast('Add a folder to scan first', { tone: 'warn' });
    return;
  }
  const tool = state.tool;
  setScanning(true);
  try {
    const result = await tauri.core.invoke('scan', { request: scanRequest() });
    if (state.tool !== tool) return; // the user switched tools while scanning
    setResults(result.groups, result.warnings, true);
    if (result.stopped) mwp.toast('Scan stopped, results may be incomplete', { tone: 'warn' });
  } catch (error) {
    mwp.toast(`Scan failed: ${error}`, { tone: 'bad' });
  } finally {
    setScanning(false);
  }
}

function setScanning(on) {
  state.scanning = on;
  if (on) $('scan').setAttribute('aria-busy', 'true');
  else $('scan').removeAttribute('aria-busy');
  $('stop').disabled = !on;
  $('progress-field').hidden = !on;
  if (on) {
    $('progress-label').textContent = 'Starting…';
    $('progress').removeAttribute('value');
  }
}

function showProgress({ payload }) {
  if (!state.scanning) return;
  $('progress-label').textContent = payload.label;
  if (payload.percent == null) $('progress').removeAttribute('value');
  else $('progress').value = payload.percent;
}

// ---- results ----

function setResults(groups, warnings, scanned) {
  // Single-file groups are not duplicates; flat tools send one group that may be empty.
  state.groups = groups.filter((group) => group.length > (TOOLS[state.tool].grouped ? 1 : 0));
  state.selected.clear();
  renderWarnings(warnings, 'Some items could not be read');
  if (scanned) renderResults(TOOLS[state.tool].none, 'Nothing matched in the chosen folders.');
  else renderResults('No scan yet', 'Add a folder and press Scan.');
}

function renderWarnings(warnings, title) {
  $('warnings').hidden = !warnings.length;
  $('warnings-title').textContent = title;
  const items = warnings.slice(0, 10).map((text) => el('li', { text }));
  if (warnings.length > 10) items.push(el('li', { text: `…and ${warnings.length - 10} more` }));
  $('warnings-list').replaceChildren(...items);
}

/** Draws state.groups; the empty texts show when there is nothing to list. */
function renderResults(emptyTitle, emptySub) {
  const tool = TOOLS[state.tool];
  const count = state.groups.reduce((n, group) => n + group.length, 0);
  $('results-empty').hidden = count > 0;
  $('results-wrap').hidden = count === 0;
  $('select-button').disabled = count === 0;
  $('empty-title').textContent = emptyTitle;
  $('empty-sub').textContent = emptySub;
  $('results-title').textContent = count ? resultsTitle(count) : 'Results';

  const table = $('results-table');
  for (const body of [...table.tBodies]) body.remove();
  // ponytail: renders every row; virtualise if result sets beyond ~20k rows get slow.
  state.groups.forEach((group, index) => table.append(renderGroup(group, index, tool.grouped)));
  updateSelection();
}

function resultsTitle(count) {
  if (!TOOLS[state.tool].grouped) return plural(count, 'item', 'items');
  const spare = state.groups.reduce((n, group) => n + group.slice(1).reduce((m, item) => m + item.size, 0), 0);
  return `${plural(count, 'file', 'files')} in ${plural(state.groups.length, 'group', 'groups')}, ${formatSize(spare)} in extra copies`;
}

function renderGroup(group, index, grouped) {
  const body = el('tbody');
  if (grouped) {
    const label = `Group ${index + 1}: ${plural(group.length, 'file', 'files')} of ${formatSize(group[0].size)}`;
    body.append(el('tr', {}, el('th', { colspan: '6', scope: 'rowgroup', text: label })));
  }
  for (const item of group) body.append(renderRow(item));
  return body;
}

function renderRow(item) {
  const { name, folder } = splitPath(item.path);
  const checkbox = el('input', { type: 'checkbox', 'aria-label': `Select ${item.path}` });
  checkbox.checked = state.selected.has(item.path);
  const size = state.tool === 'empty-folders' ? el('span', { 'aria-hidden': 'true', text: '—' }) : formatSize(item.size);
  return el('tr', { 'data-path': item.path },
    el('td', {}, checkbox),
    el('td', { text: name }),
    el('td', {}, el('span', { class: 'mwp-muted', text: folder })),
    el('td', { class: 'mwp-table__num', 'data-sort': String(item.size) }, size),
    el('td', { text: formatDate(item.modifiedDate) }),
    el('td', {}, el('button', { type: 'button', class: 'mwp-btn mwp-btn--ghost mwp-btn--icon', 'data-size': 's', 'data-reveal': '', 'aria-label': `Show ${name} in Explorer` }, icon('external-link'))));
}

function onTableChange(event) {
  const row = event.target.closest('tr[data-path]');
  if (!row || event.target.type !== 'checkbox') return;
  if (event.target.checked) state.selected.add(row.dataset.path);
  else state.selected.delete(row.dataset.path);
  updateSelection();
}

function onTableClick(event) {
  const button = event.target.closest('[data-reveal]');
  if (!button) return;
  const { path } = button.closest('tr').dataset;
  tauri.opener.revealItemInDir(path).catch((error) => mwp.toast(`Cannot show ${splitPath(path).name}: ${error}`, { tone: 'bad' }));
}

function applySelection(mode) {
  state.selected.clear();
  for (const group of state.groups) {
    if (mode === 'all') {
      for (const item of group) state.selected.add(item.path);
    } else if (mode === 'all-but-newest' || mode === 'all-but-oldest') {
      const newer = (a, b) => (mode === 'all-but-newest' ? a.modifiedDate > b.modifiedDate : a.modifiedDate < b.modifiedDate);
      const keep = group.reduce((best, item) => (newer(item, best) ? item : best));
      for (const item of group) if (item !== keep) state.selected.add(item.path);
    }
  }
  for (const box of $('results-table').querySelectorAll('tbody input[type="checkbox"]')) {
    box.checked = state.selected.has(box.closest('tr').dataset.path);
  }
  updateSelection();
}

function updateSelection() {
  const count = state.selected.size;
  const button = $('trash');
  button.disabled = count === 0;
  button.replaceChildren(icon('trash-2'), count ? ` Move ${count} to Recycle Bin` : ' Move to Recycle Bin');
}

async function trashSelected() {
  const tool = TOOLS[state.tool];
  const items = state.groups.flat().filter((item) => state.selected.has(item.path));
  const size = formatSize(items.reduce((n, item) => n + item.size, 0));
  const lostGroups = tool.grouped ? state.groups.filter((group) => group.every((item) => state.selected.has(item.path))).length : 0;
  const warning = lostGroups ? `${plural(lostGroups, 'group loses', 'groups lose')} every copy. ` : '';
  const ok = await mwp.confirm({
    title: `Move ${plural(items.length, 'item', 'items')} to the Recycle Bin?`,
    body: `${warning}${items.length === 1 ? 'It takes' : 'They take'} ${size}. You can restore ${items.length === 1 ? 'it' : 'them'} from the Recycle Bin.`,
    danger: 'Move to Recycle Bin',
  });
  if (!ok) return;

  const button = $('trash');
  button.setAttribute('aria-busy', 'true');
  try {
    const failures = await tauri.core.invoke('move_to_trash', { paths: items.map((item) => item.path) });
    const failed = new Set(failures.map((failure) => failure.path));
    const moved = items.filter((item) => !failed.has(item.path));
    const movedPaths = new Set(moved.map((item) => item.path));
    state.groups = state.groups
      .map((group) => group.filter((item) => !movedPaths.has(item.path)))
      .filter((group) => group.length > (tool.grouped ? 1 : 0));
    // A group left with one file is gone from the table, so its file must not stay selected.
    const shown = new Set(state.groups.flat().map((item) => item.path));
    for (const path of [...state.selected]) if (!shown.has(path)) state.selected.delete(path);
    renderResults('Nothing left to clean', 'Everything you selected is in the Recycle Bin.');
    if (moved.length) {
      const freed = formatSize(moved.reduce((n, item) => n + item.size, 0));
      mwp.toast(`Moved ${plural(moved.length, 'item', 'items')} to the Recycle Bin, ${freed} freed`, { tone: 'good' });
    }
    renderWarnings(failures.map((failure) => `${failure.path}: ${failure.error}`), 'Some items could not be moved');
  } catch (error) {
    mwp.toast(`Moving to the Recycle Bin failed: ${error}`, { tone: 'bad' });
  } finally {
    button.removeAttribute('aria-busy');
  }
}

// ---- wiring ----

renderFolders('included');
renderFolders('excluded');
$('add-included').addEventListener('click', () => addFolders('included'));
$('add-excluded').addEventListener('click', () => addFolders('excluded'));
for (const kind of ['included', 'excluded']) {
  $(`${kind}-list`).addEventListener('click', (event) => {
    const button = event.target.closest('[data-remove]');
    if (button) removeFolder(kind, button.dataset.remove);
  });
}
$('scan').addEventListener('click', scan);
$('stop').addEventListener('click', () => tauri.core.invoke('stop_scan'));
$('trash').addEventListener('click', trashSelected);
$('select-menu').addEventListener('click', (event) => {
  const item = event.target.closest('[data-select]');
  if (item) applySelection(item.dataset.select);
});
$('results-table').addEventListener('change', onTableChange);
$('results-table').addEventListener('click', onTableClick);
window.addEventListener('hashchange', showTool);
tauri.event.listen('scan-progress', showProgress);
showTool();
