import { mwp } from './vendor/mwp-ui/mwp.js';

const tauri = window.__TAURI__;

const LIST_COLUMNS = ['select', 'name', 'folder', 'size', 'modified', 'show'];
const TOOLS = {
  'duplicates': { title: 'Duplicates', sub: 'Which files are stored more than once?', grouped: true, copies: true, none: 'No duplicates found', columns: LIST_COLUMNS },
  'similar-images': {
    title: 'Similar images', sub: 'Which pictures look alike?', grouped: true, noun: ['image', 'images'], none: 'No similar images found',
    columns: ['select', 'preview', 'name', 'folder', 'resolution', 'similarity', 'size', 'modified', 'show'],
  },
  'big-files': { title: 'Big files', sub: 'Which files take the most space?', grouped: false, none: 'No files found', columns: LIST_COLUMNS },
  'empty-folders': { title: 'Empty folders', sub: 'Which folders hold nothing?', grouped: false, none: 'No empty folders found', columns: LIST_COLUMNS },
  'empty-files': { title: 'Empty files', sub: 'Which files are 0 bytes?', grouped: false, none: 'No empty files found', columns: LIST_COLUMNS },
  'disk-usage': { title: 'Disk usage', sub: 'What takes up the space?', disk: true },
};
const FOLDERS_KEY = 'plekke.folders';
const NUMBER = new Intl.NumberFormat(document.documentElement.lang, { maximumFractionDigits: 1 });
/** Longest side of a preview on screen; the backend renders it at twice that for HiDPI. */
const PREVIEW_PX = 96;
/** Bars in the disk usage chart before the rest is folded into one. */
const CHART_ROWS = 10;

const $ = (id) => document.getElementById(id);
// thumbs: image path -> blob URL of its preview, freed when the results are replaced
const state = { tool: 'duplicates', groups: [], selected: new Set(), scanning: false, thumbs: new Map() };
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

/** The row's path is read from its `tr[data-path]` when clicked. */
function revealButton(name) {
  return el('button', { type: 'button', class: 'mwp-btn mwp-btn--ghost mwp-btn--icon', 'data-size': 's', 'data-reveal': '', 'aria-label': `Show ${name} in Explorer` }, icon('external-link'));
}

function revealRow(button) {
  const { path } = button.closest('tr').dataset;
  tauri.opener.revealItemInDir(path).catch((error) => mwp.toast(`Cannot show ${splitPath(path).name}: ${error}`, { tone: 'bad' }));
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
  document.title = `${tool.title} · Plekke`;
  for (const block of document.querySelectorAll('[data-options]')) block.hidden = block.dataset.options !== state.tool;
  for (const item of document.querySelectorAll('#select-menu [data-select^="all-but"], #select-menu hr')) {
    item.hidden = !tool.grouped;
    item.disabled = !tool.grouped;
  }
  $('results-card').hidden = Boolean(tool.disk);
  $('disk-view').hidden = !tool.disk;
  if (tool.columns) {
    $('results-head').replaceChildren(el('tr', {}, ...tool.columns.map((key) => {
      const column = COLUMNS[key];
      return el('th', { class: column.num ? 'mwp-table__num' : null }, typeof column.head === 'function' ? column.head() : column.head);
    })));
  }
  setResults([], [], false);
  resetDisk();
}

function scanRequest() {
  const request = { included: folders.included, excluded: folders.excluded };
  if (state.tool === 'duplicates') {
    const minimalFileSize = Math.max(0, Math.round(Number($('dup-min-size').value || 0) * 1000));
    request.tool = { kind: 'duplicates', method: $('dup-method').value, minimalFileSize };
  } else if (state.tool === 'similar-images') {
    request.tool = { kind: 'similarImages', similarity: $('img-similarity').value };
  } else if (state.tool === 'disk-usage') {
    request.tool = { kind: 'diskUsage' };
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
    if (TOOLS[tool].disk) {
      renderWarnings(result.warnings, 'Some items could not be read');
      const overview = await tauri.core.invoke('disk_folder', { address: [] });
      if (overview.entries.length === 1) await openDiskFolder([0]);
      else renderDisk(overview);
    } else {
      setResults(result.groups, result.warnings, true);
    }
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
  clearThumbnails();
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
  state.groups.forEach((group, index) => table.append(renderGroup(group, index, tool)));
  updateSelection();
}

function resultsTitle(count) {
  const tool = TOOLS[state.tool];
  if (!tool.grouped) return plural(count, 'item', 'items');
  const groups = plural(state.groups.length, 'group', 'groups');
  if (!tool.copies) return `${plural(count, ...tool.noun)} in ${groups}`;
  const spare = state.groups.reduce((n, group) => n + group.slice(1).reduce((m, item) => m + item.size, 0), 0);
  return `${plural(count, 'file', 'files')} in ${groups}, ${formatSize(spare)} in extra copies`;
}

function renderGroup(group, index, tool) {
  const body = el('tbody');
  if (tool.grouped) {
    const what = tool.copies ? `${plural(group.length, 'file', 'files')} of ${formatSize(group[0].size)}` : plural(group.length, ...tool.noun);
    body.append(el('tr', {}, el('th', { colspan: String(tool.columns.length), scope: 'rowgroup', text: `Group ${index + 1}: ${what}` })));
  }
  for (const item of group) body.append(renderRow(item, tool));
  return body;
}

/** Table columns; each tool lists the ones it shows. A cell returns a node or a string, never markup. */
const COLUMNS = {
  select: {
    head: () => el('span', { class: 'mwp-muted', text: 'Select' }),
    cell: (item) => {
      const checkbox = el('input', { type: 'checkbox', 'aria-label': `Select ${item.path}` });
      checkbox.checked = state.selected.has(item.path);
      return checkbox;
    },
  },
  preview: { head: () => el('span', { class: 'mwp-muted', text: 'Preview' }), cell: previewImage },
  name: { head: 'Name', cell: (item) => splitPath(item.path).name },
  folder: { head: 'Folder', cell: (item) => el('span', { class: 'mwp-muted', text: splitPath(item.path).folder }) },
  resolution: { head: 'Resolution', num: true, cell: (item) => `${item.image.width} × ${item.image.height}` },
  similarity: { head: 'Similarity', cell: (item) => item.image.similarity },
  size: { head: 'Size', num: true, cell: (item) => (state.tool === 'empty-folders' ? el('span', { 'aria-hidden': 'true', text: '—' }) : formatSize(item.size)) },
  modified: { head: 'Modified', cell: (item) => formatDate(item.modifiedDate) },
  show: { head: () => el('span', { class: 'mwp-muted', text: 'Show' }), cell: (item) => revealButton(splitPath(item.path).name) },
};

function renderRow(item, tool) {
  return el('tr', { 'data-path': item.path }, ...tool.columns.map((key) => {
    const column = COLUMNS[key];
    return el('td', { class: column.num ? 'mwp-table__num' : null }, column.cell(item));
  }));
}

// ---- image previews ----

/** Previews load once their row scrolls near the screen, so a big result does not decode every image at once. */
const thumbObserver = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    thumbObserver.unobserve(entry.target);
    loadThumbnail(entry.target);
  }
}, { rootMargin: '300px' });

function previewImage(item) {
  const { width, height } = item.image;
  const scale = PREVIEW_PX / Math.max(width, height, 1);
  const box = width && height ? [Math.max(1, Math.round(width * scale)), Math.max(1, Math.round(height * scale))] : [PREVIEW_PX, PREVIEW_PX];
  const img = el('img', { alt: '', decoding: 'async', width: String(box[0]), height: String(box[1]), 'data-thumb': item.path });
  const cached = state.thumbs.get(item.path);
  if (cached) img.src = cached;
  else thumbObserver.observe(img);
  return img;
}

async function loadThumbnail(img) {
  const path = img.dataset.thumb;
  try {
    const png = await tauri.core.invoke('thumbnail', { path });
    if (!img.isConnected) return; // results were replaced meanwhile
    const url = URL.createObjectURL(new Blob([png], { type: 'image/png' }));
    state.thumbs.set(path, url);
    img.src = url;
  } catch {
    img.replaceWith(el('span', { class: 'mwp-muted', text: 'No preview' }));
  }
}

function clearThumbnails() {
  thumbObserver.disconnect();
  for (const url of state.thumbs.values()) URL.revokeObjectURL(url);
  state.thumbs.clear();
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
  if (button) revealRow(button);
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
  const warning = lostGroups ? `${plural(lostGroups, 'group loses', 'groups lose')} every ${tool.copies ? 'copy' : 'image'}. ` : '';
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

// ---- disk usage ----

function resetDisk() {
  $('disk-empty').hidden = false;
  $('disk-chart-card').hidden = true;
  $('disk-card').hidden = true;
  $('disk-crumbs').hidden = true;
}

async function openDiskFolder(address) {
  try {
    renderDisk(await tauri.core.invoke('disk_folder', { address }));
  } catch (error) {
    mwp.toast(`Cannot open this folder: ${error}`, { tone: 'bad' });
  }
}

function renderDisk(folder) {
  const here = folder.crumbs.at(-1)?.name ?? 'Scanned folders';
  const count = folder.entries.length + folder.moreEntries;
  $('disk-empty').hidden = true;
  $('disk-card').hidden = false;
  $('disk-chart-card').hidden = !folder.entries.some((entry) => entry.size > 0);
  $('disk-title').textContent = `${formatSize(folder.size)} in ${plural(count, 'item', 'items')}`;
  $('disk-chart-caption').textContent = `Biggest items in ${here}`;
  $('disk-chart-rows').replaceChildren(...diskChartRows(folder));
  renderCrumbs(folder.crumbs);

  const rows = folder.entries.map((entry) => diskRow(entry, folder.size));
  if (folder.moreEntries) {
    const text = `…and ${plural(folder.moreEntries, 'more item', 'more items')}, ${formatSize(folder.moreSize)}`;
    rows.push(el('tr', {}, el('td', { colspan: '5' }, el('span', { class: 'mwp-muted', text }))));
  }
  if (!rows.length) rows.push(el('tr', {}, el('td', { colspan: '5' }, el('span', { class: 'mwp-muted', text: 'This folder is empty' }))));
  $('disk-rows').replaceChildren(...rows);
}

function renderCrumbs(crumbs) {
  const trail = [{ name: 'Scanned folders', address: [] }, ...crumbs];
  $('disk-crumbs').replaceChildren(...trail.map((crumb, index) => (index === trail.length - 1
    ? el('span', { class: 'mwp-crumbs__item', 'aria-current': 'page', text: crumb.name })
    : el('a', { class: 'mwp-crumbs__item', href: '#disk-usage', 'data-address': crumb.address.join(','), text: crumb.name }))));
  $('disk-crumbs').hidden = false;
}

/** The biggest entries as bars, the rest folded into one; entries come sorted biggest first. */
function diskChartRows(folder) {
  const bar = (label, size) => el('tr', {},
    el('th', { scope: 'row', text: label }),
    el('td', { class: 'mwp-table__num' }, el('data', { value: String(size), text: formatSize(size) })));
  const shown = folder.entries.slice(0, CHART_ROWS).filter((entry) => entry.size > 0);
  const rows = shown.map((entry) => bar(entry.name, entry.size));
  const restCount = folder.entries.length - shown.length + folder.moreEntries;
  const restSize = folder.entries.slice(shown.length).reduce((n, entry) => n + entry.size, folder.moreSize);
  if (restSize > 0) rows.push(bar(`Everything else (${plural(restCount, 'item', 'items')})`, restSize));
  return rows;
}

function diskRow(entry, folderSize) {
  const name = entry.kind === 'directory'
    ? el('a', { href: '#disk-usage', 'data-address': entry.address.join(','), text: entry.name })
    : entry.name;
  const share = folderSize ? `${NUMBER.format((entry.size / folderSize) * 100)} %` : '';
  return el('tr', { 'data-path': entry.path },
    el('td', {}, name),
    el('td', {}, diskKind(entry)),
    el('td', { class: 'mwp-table__num' }, formatSize(entry.size)),
    el('td', { class: 'mwp-table__num' }, share),
    el('td', {}, revealButton(entry.name)));
}

function diskKind(entry) {
  if (entry.kind === 'directory') return `Folder, ${plural(entry.items, 'item', 'items')}`;
  if (entry.kind === 'symlink') return 'Link';
  if (entry.kind === 'inaccessible') return el('span', { class: 'mwp-badge', 'data-tone': 'warn', text: 'No access' });
  return entry.category;
}

/** Folder links in the breadcrumb and the table open that folder; `data-address` is empty for the overview. */
function onDiskClick(event) {
  const reveal = event.target.closest('[data-reveal]');
  if (reveal) {
    revealRow(reveal);
    return;
  }
  const link = event.target.closest('a[data-address]');
  if (!link) return;
  event.preventDefault();
  openDiskFolder(link.dataset.address ? link.dataset.address.split(',').map(Number) : []);
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
$('disk-table').addEventListener('click', onDiskClick);
$('disk-crumbs').addEventListener('click', onDiskClick);
window.addEventListener('hashchange', showTool);
tauri.event.listen('scan-progress', showProgress);
showTool();
