// mwp.js: the behaviour the platform lacks. Everything else is CSS.
// Elements: <mwp-tabs> <mwp-mode-toggle> <mwp-menu> <mwp-sort> <mwp-chart> <mwp-toast-host> (created on load, stays open: the live region)
// API:      mwp.toast(message, { tone, action, duration })
//           mwp.announce(message)  screen readers only, through the same live region
//           mwp.confirm({ title, body, ok, cancel, danger }) -> Promise<boolean>
//           mwp.mode(value?)  mwp.brand(value?)  mwp.palette(value?)   persist to localStorage and apply before first paint
// Only aria defaults are user-facing strings; each is overridable with an attribute or option.

const html = document.documentElement;
const MODE_KEY = 'mwp-mode';
const BRAND_KEY = 'mwp-brand';
const PALETTE_KEY = 'mwp-palette';
const MODES = ['system', 'light', 'dark'];

const ICONS = {
  system: '<svg class="mwp-icon" viewBox="0 0 24 24" aria-hidden="true"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/></svg>',
  light: '<svg class="mwp-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
  dark: '<svg class="mwp-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"/></svg>',
};
let seq = 0;

const toastHost = () => {
  let host = document.querySelector('mwp-toast-host');
  if (!host) { host = el('mwp-toast-host', { popover: 'manual' }); document.body.append(host); }
  return host;
};
const toastMs = () => parseFloat(getComputedStyle(html).getPropertyValue('--mwp-duration-toast')) || 5000;

const el = (tag, attrs = {}, children = []) => {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) if (v != null) node.setAttribute(k, v);
  for (const c of children) node.append(c);
  return node;
};

export const mwp = {
  mode(value) {
    if (value === undefined) return html.dataset.mode || 'system';
    html.dataset.mode = value;
    try { localStorage.setItem(MODE_KEY, value); } catch {}
    html.querySelectorAll('mwp-mode-toggle').forEach((t) => t.render());
    return value;
  },
  brand(value) {
    if (value === undefined) return html.dataset.brand || 'ink';
    html.dataset.brand = value;
    try { localStorage.setItem(BRAND_KEY, value); } catch {}
    return value;
  },
  // '' is the brand's own palette: no attribute, nothing stored, so it follows a later brand change.
  palette(value) {
    if (value === undefined) return html.dataset.palette || '';
    if (value) html.dataset.palette = value; else delete html.dataset.palette;
    try { if (value) localStorage.setItem(PALETTE_KEY, value); else localStorage.removeItem(PALETTE_KEY); } catch {}
    return value || '';
  },
  toast(message, { tone, action, duration } = {}) {
    const host = toastHost();
    const toast = el('div', { class: 'mwp-toast', 'data-tone': tone }, [message]);
    if (action) {
      const btn = el('button', { class: 'mwp-toast__action', type: 'button' }, [action.label]);
      btn.addEventListener('click', () => { action.onClick?.(); remove(); });
      toast.append(btn);
    }
    const ms = duration ?? toastMs();
    // The timer pauses while the pointer is over the toast or focus is inside it (the action button), so a
    // keyboard user can reach the action before it goes.
    let timer = setTimeout(remove, ms);
    let hover = false, focus = false;
    const pause = () => clearTimeout(timer);
    const resume = () => { clearTimeout(timer); if (!hover && !focus && toast.isConnected) timer = setTimeout(remove, ms); };
    toast.addEventListener('mouseenter', () => { hover = true; pause(); });
    toast.addEventListener('focusin', () => { focus = true; pause(); });
    toast.addEventListener('mouseleave', () => { hover = false; resume(); });
    toast.addEventListener('focusout', () => { focus = false; resume(); });
    function remove() {
      clearTimeout(timer);
      toast.remove();
    }
    host.append(toast);
    return remove;
  },
  announce(message) {
    const node = el('span', { class: 'mwp-announce' }, [message]);
    toastHost().append(node);
    setTimeout(() => node.remove(), toastMs());
  },
  confirm({ title, body, ok = 'OK', cancel = 'Cancel', danger } = {}) {
    const titleId = `mwp-confirm-title-${++seq}`;
    const cancelBtn = el('button', { class: 'mwp-btn mwp-btn--ghost', type: 'button', autofocus: danger ? '' : null }, [cancel]);
    const dialog = el('dialog', { class: 'mwp-dialog', 'aria-labelledby': titleId, closedby: 'any' }, [
      el('form', { method: 'dialog', class: 'mwp-dialog__form' }, [
        el('header', { class: 'mwp-dialog__head' }, [el('h2', { class: 'mwp-dialog__title', id: titleId }, [title ?? ''])]),
        body ? el('div', { class: 'mwp-dialog__body' }, [body]) : '',
        el('footer', { class: 'mwp-dialog__foot' }, [
          cancelBtn,
          el('button', { class: `mwp-btn ${danger ? 'mwp-btn--danger' : 'mwp-btn--primary'}`, value: 'ok', type: 'submit', autofocus: danger ? null : '' }, [danger ?? ok]),
        ]),
      ]),
    ]);
    cancelBtn.addEventListener('click', () => dialog.close());
    document.body.append(dialog);
    dialog.showModal();
    return new Promise((resolve) => dialog.addEventListener('close', () => { resolve(dialog.returnValue === 'ok'); dialog.remove(); }, { once: true }));
  },
};

// Tabs: click and arrow keys select; panels follow via hidden.
class Tabs extends HTMLElement {
  connectedCallback() {
    this.addEventListener('click', (e) => { const tab = e.target.closest('[role="tab"]'); if (tab && this.contains(tab)) this.select(tab); });
    this.addEventListener('keydown', (e) => {
      const tabs = this.tabs;
      const i = tabs.indexOf(e.target);
      if (i < 0) return;
      const next = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
      if (next === undefined) return;
      e.preventDefault();
      const tab = tabs.at(next % tabs.length);
      tab.focus();
      this.select(tab);
    });
    this.select(this.querySelector('[role="tab"][aria-selected="true"]') ?? this.tabs[0]);
  }
  get tabs() { return [...this.querySelectorAll('[role="tab"]')]; }
  select(selected) {
    for (const tab of this.tabs) {
      const on = tab === selected;
      tab.setAttribute('aria-selected', on);
      tab.tabIndex = on ? 0 : -1;
      const panel = this.querySelector(`#${CSS.escape(tab.getAttribute('aria-controls') ?? '')}`);
      if (panel) panel.hidden = !on;
    }
  }
}

// Mode toggle: cycles system -> light -> dark, persists. Label prefix overridable with the label attribute.
class ModeToggle extends HTMLElement {
  connectedCallback() {
    this.button = el('button', { type: 'button', class: 'mwp-btn mwp-btn--ghost mwp-btn--icon' });
    this.button.addEventListener('click', () => mwp.mode(MODES[(MODES.indexOf(mwp.mode()) + 1) % MODES.length]));
    this.replaceChildren(this.button);
    this.render();
  }
  render() {
    const mode = mwp.mode();
    this.button.innerHTML = ICONS[mode] ?? ICONS.system;
    this.button.setAttribute('aria-label', `${this.getAttribute('label') ?? 'Colour mode'}: ${mode}`);
  }
}

// Menu: the ARIA menu button pattern on a popover. The element sets the roles; the markup stays plain
// buttons, links and <hr>. Arrow keys, Home/End and typeahead move focus, Tab and a choice close it.
// An item with aria-checked becomes menuitemcheckbox; write role="menuitemradio" for one-of-many.
class Menu extends HTMLElement {
  constructor() {
    super();
    this.addEventListener('toggle', (e) => {
      if (e.newState !== 'open') return;
      this.prepare();
      const trigger = this.trigger;
      if (trigger) {
        const r = trigger.getBoundingClientRect();
        const gap = parseFloat(getComputedStyle(html).getPropertyValue('--mwp-space-1')) || 4;
        // The rect is physical, so the inset is too; a logical inset would mirror in RTL.
        this.style.top = `${Math.min(r.bottom + gap, window.innerHeight - this.offsetHeight - gap)}px`;
        this.style.left = `${Math.min(r.left, window.innerWidth - this.offsetWidth - gap)}px`;
      }
      this.items.at(this.focusLast ? -1 : 0)?.focus();
      this.focusLast = false;
    });
    let typed = '';
    let typedTimer;
    this.addEventListener('keydown', (e) => {
      if (e.key === 'Tab') { this.hidePopover(); return; }
      const items = this.items;
      const i = items.indexOf(document.activeElement);
      const next = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: items.length - 1 }[e.key];
      if (next !== undefined) { e.preventDefault(); items.at(next % items.length)?.focus(); return; }
      if (e.key === ' ' && !typed && document.activeElement?.matches('a.mwp-menu__item')) { e.preventDefault(); document.activeElement.click(); return; }
      if (e.key.length !== 1 || e.ctrlKey || e.metaKey || e.altKey || (e.key === ' ' && !typed)) return;
      e.preventDefault();
      clearTimeout(typedTimer);
      typedTimer = setTimeout(() => (typed = ''), parseFloat(getComputedStyle(html).getPropertyValue('--mwp-duration-typeahead')) || 500);
      typed += e.key.toLowerCase();
      const run = [...typed].every((c) => c === typed[0]);
      const needle = run ? typed[0] : typed;
      const from = run ? i + 1 : i;
      [...items.slice(from), ...items.slice(0, from)].find((it) => it.textContent.trim().toLowerCase().startsWith(needle))?.focus();
    });
    this.addEventListener('click', (e) => { if (e.target.closest('.mwp-menu__item')) this.hidePopover(); });
  }
  connectedCallback() {
    this.setAttribute('role', 'menu');
    this.prepare();
  }
  // Roles and tabindex, at connect and on every open so items added later are covered too.
  prepare() {
    for (const item of this.querySelectorAll('.mwp-menu__item')) {
      if (!item.hasAttribute('role')) item.setAttribute('role', item.hasAttribute('aria-checked') ? 'menuitemcheckbox' : 'menuitem');
      item.tabIndex = -1;
    }
    const trigger = this.trigger;
    if (!trigger) return;
    trigger.setAttribute('aria-haspopup', 'menu');
    if (!this.hasAttribute('aria-labelledby')) {
      if (!trigger.id) trigger.id = `${this.id}-trigger`;
      this.setAttribute('aria-labelledby', trigger.id);
    }
  }
  get trigger() { return document.querySelector(`[popovertarget="${CSS.escape(this.id)}"]`); }
  get items() { return [...this.querySelectorAll('.mwp-menu__item:not(:disabled)')]; }
}

// Sort: wraps a table. A click on a .mwp-table__sort button in a <th> reorders the rows of the first <tbody> by that
// column, sets aria-sort on the th (the CSS draws the arrow) and announces the order, since screen readers do not
// read aria-sort changes. A cell sorts by its data-sort when present (ISO dates, raw numbers), else by its text;
// plain decimals compare as numbers. A __detail row travels with the row before it. Header cells must not span
// columns. Words overridable: <mwp-sort ascending="…" descending="…">.
class Sort extends HTMLElement {
  constructor() {
    super();
    this.addEventListener('click', (e) => {
      const button = e.target.closest('.mwp-table__sort');
      const th = button?.closest('th');
      if (!th || !this.contains(th)) return;
      const dir = th.getAttribute('aria-sort') === 'ascending' ? 'descending' : 'ascending';
      const table = th.closest('table');
      for (const other of table.querySelectorAll('th[aria-sort]')) other.removeAttribute('aria-sort');
      th.setAttribute('aria-sort', dir);
      const i = th.cellIndex;
      const body = table.tBodies[0];
      const key = (row) => { const cell = row.cells[i]; return cell?.dataset.sort ?? cell?.textContent.trim() ?? ''; };
      const num = (v) => (/^-?\d+(\.\d+)?$/.test(v) ? +v : NaN);
      const sign = dir === 'ascending' ? 1 : -1;
      const groups = [...body.rows].filter((r) => !r.classList.contains('mwp-table__detail'))
        .map((r) => [r, ...(r.nextElementSibling?.classList.contains('mwp-table__detail') ? [r.nextElementSibling] : [])]);
      groups.sort(([a], [b]) => {
        const [x, y] = [key(a), key(b)];
        const [nx, ny] = [num(x), num(y)];
        return sign * (Number.isNaN(nx) || Number.isNaN(ny) ? x.localeCompare(y, undefined, { numeric: true, sensitivity: 'base' }) : nx - ny);
      });
      body.append(...groups.flat());
      mwp.announce(`${button.textContent.trim()}, ${this.getAttribute(dir) ?? dir}`);
    });
  }
}

// Chart: wraps a table and draws it. Rows are categories or points in time (the first cell names them), every other
// column is a series. A cell's value is its <data value>, else its text when that is a plain number; the cell text is
// the label shown, so the server formats it. type: bar (default, horizontal bars with a value column), column (series
// stacked, negatives below zero), line (a line per series), sparkline (the first series as a bare line). Column and
// line get a value axis whose ticks Intl.NumberFormat writes in the page's lang, the unit attribute once above it, and
// a tooltip on hover and on the arrow keys: the chart is one tab stop. data-series on a header pins a colour. The
// drawing is aria-hidden: the table stays for screen readers and is redrawn when it changes.
const SVG = 'http://www.w3.org/2000/svg';
const svg = (tag, attrs) => { const node = document.createElementNS(SVG, tag); for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v); return node; };
// About five round steps (1, 2, 2.5, 5 × 10^k) that cover lo..hi, and the significant digits that write every one of
// them exactly (from the largest tick's first digit down to the step's last), so 250.250 never shows as 250K.
const ticks = (lo, hi) => {
  if (lo === hi) [lo, hi] = [Math.min(0, lo), Math.max(0, hi) || 1];
  const raw = (hi - lo) / 4;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const m = [1, 2, 2.5, 5, 10].find((m) => m * mag >= raw);
  const step = m * mag;
  const out = [];
  for (let k = Math.floor(lo / step); k <= Math.ceil(hi / step); k++) out.push(+(k * step).toPrecision(12));
  const last = Math.round(Math.log10(mag)) + (m === 10 ? 1 : m === 2.5 ? -1 : 0);
  const first = Math.floor(Math.log10(Math.max(...out.map(Math.abs))));
  return [out, Math.min(21, Math.max(1, first - last + 1))];
};

class Chart extends HTMLElement {
  static observedAttributes = ['type', 'unit'];
  constructor() {
    super();
    this.addEventListener('keydown', (e) => {
      if (!this.points || e.altKey || e.ctrlKey || e.metaKey) return;
      if (e.key === 'Escape') { this.show(null); return; }
      const i = this.active ?? this.points - 1;
      const ahead = getComputedStyle(this).direction === 'rtl' ? -1 : 1;
      const next = { ArrowRight: i + ahead, ArrowLeft: i - ahead, Home: 0, End: this.points - 1 }[e.key];
      if (next === undefined) return;
      e.preventDefault();
      this.show(Math.max(0, Math.min(this.points - 1, next)));
    });
    this.addEventListener('focus', () => { if (this.points) this.show(this.active ?? this.points - 1); });
    this.addEventListener('blur', () => this.show(null));
  }
  // The whole element is observed, so a replaced table redraws too; changes inside the drawing (the tooltip) and the
  // drawing's own insertion (dropped by takeRecords at the end of render) do not.
  connectedCallback() {
    this.resizer ??= new ResizeObserver(() => this.thin()); // render() points it at the area and the last row label
    this.render();
    this.observer ??= new MutationObserver((records) => { if (records.some((r) => !this.drawing?.contains(r.target))) this.render(); });
    this.observer.observe(this, { subtree: true, childList: true, characterData: true, attributes: true });
  }
  disconnectedCallback() { this.observer?.disconnect(); this.resizer?.disconnect(); }
  attributeChangedCallback() { if (this.isConnected) this.render(); }
  render() {
    const table = this.querySelector(':scope > table');
    this.resizer?.disconnect();
    this.drawing?.remove();
    this.drawing = null;
    this.points = 0;
    this.parts = null;
    this.removeAttribute('tabindex');
    this.removeAttribute('role');
    if (this.named) this.removeAttribute('aria-label');
    this.named = false;
    if (!table) return this.observer?.takeRecords();
    const type = this.getAttribute('type') ?? 'bar';
    const heads = [...(table.tHead?.rows[0]?.cells ?? [])].slice(1);
    const rows = [...table.tBodies].flatMap((body) => [...body.rows]);
    const text = (cell) => cell?.textContent.trim() ?? '';
    const num = (cell) => { const v = cell?.querySelector('data')?.value ?? text(cell); return /^-?\d+(\.\d+)?$/.test(v) ? +v : null; };
    const grid = rows.map((row) => [...row.cells].slice(1).map(num));
    const colour = (i, row) => row?.cells[0].dataset.series ?? heads[i]?.dataset.series ?? String(i + 1);
    const n = rows.length;
    if (!n) return this.observer?.takeRecords(); // nothing to draw: the tool shows an empty state instead
    const drawing = el('div', { class: 'mwp-chart', 'data-type': type, 'aria-hidden': 'true' });
    drawing.style.setProperty('--_n', n);
    if (heads.length > 1 && type !== 'sparkline')
      drawing.append(el('div', { class: 'mwp-chart__legend' }, heads.map((h, i) => el('span', { class: 'mwp-chart__key', 'data-series': colour(i) }, [text(h)]))));

    if (type === 'bar') {
      const all = grid.flat().map((v) => v ?? 0);
      const lo = Math.min(0, ...all);
      const span = Math.max(0, ...all) - lo || 1;
      drawing.style.setProperty('--_zero', -lo / span);
      drawing.append(el('div', { class: 'mwp-chart__plot' }, rows.flatMap((row, r) => [
        el('span', { class: 'mwp-chart__label' }, [text(row.cells[0])]),
        el('span', { class: 'mwp-chart__track' }, grid[r].map((v, i) => {
          const bar = el('span', { class: 'mwp-chart__bar', 'data-series': colour(i, row), 'data-sign': v < 0 ? 'negative' : null });
          bar.style.setProperty('--_start', (Math.min(v ?? 0, 0) - lo) / span);
          bar.style.setProperty('--_size', Math.abs(v ?? 0) / span);
          return bar;
        })),
        el('span', { class: 'mwp-chart__value' }, [...row.cells].slice(1).map((cell) => el('span', {}, [text(cell)]))),
      ])));
    } else {
      const spark = type === 'sparkline';
      const series = spark ? [0] : heads.map((_, i) => i);
      let lo, hi;
      if (type === 'column') {
        lo = Math.min(0, ...grid.map((r) => r.reduce((sum, v) => sum + Math.min(v ?? 0, 0), 0)));
        hi = Math.max(0, ...grid.map((r) => r.reduce((sum, v) => sum + Math.max(v ?? 0, 0), 0)));
      } else {
        const all = grid.flatMap((r) => series.map((i) => r[i])).filter((v) => v != null);
        [lo, hi] = [Math.min(...all), Math.max(...all)];
      }
      const [steps, digits] = spark ? [[lo, hi]] : ticks(lo, hi);
      const [a, span] = [steps[0], steps.at(-1) - steps[0] || 1];
      const y = (v) => (v - a) / span;
      const x = (k) => (k + 0.5) / n;
      const at = (node, props) => { for (const [k, v] of Object.entries(props)) node.style.setProperty(k, v); return node; };
      const area = el('div', { class: 'mwp-chart__area' });
      if (!spark) area.append(...steps.map((v) => at(el('span', { class: 'mwp-chart__grid', 'data-zero': v === 0 ? '' : null }), { '--_y': y(v) })));
      const guide = el('span', { class: 'mwp-chart__guide', hidden: '' });
      const tip = el('div', { class: 'mwp-chart__tip', hidden: '' });
      if (!spark) area.append(guide);
      if (type === 'column') {
        area.append(...grid.map((r) => {
          const col = el('span', { class: 'mwp-chart__col' });
          let up = 0, down = 0, top, bottom;
          r.forEach((v, i) => {
            if (!v) return;
            const seg = at(el('span', { class: 'mwp-chart__seg', 'data-series': colour(i) }), { '--_start': y(v > 0 ? up : down + v), '--_size': Math.abs(v) / span });
            if (v > 0) { up += v; top = seg; } else { down += v; bottom = seg; }
            col.append(seg);
          });
          top?.setAttribute('data-end', 'up');
          bottom?.setAttribute('data-end', 'down');
          return col;
        }));
      } else {
        const lines = svg('svg', { class: 'mwp-chart__lines', viewBox: '0 0 100 100', preserveAspectRatio: 'none' });
        const dots = [];
        for (const i of series) {
          const runs = [[]]; // a missing value breaks the line instead of bridging it
          grid.forEach((r, k) => { if (r[i] != null) runs.at(-1).push([k, r[i]]); else if (runs.at(-1).length) runs.push([]); });
          for (const run of runs) lines.append(svg('polyline', { class: 'mwp-chart__line', 'data-series': colour(i), points: run.map(([k, v]) => `${x(k) * 100},${100 - y(v) * 100}`).join(' ') }));
          const [k, v] = runs.flat().at(-1) ?? [];
          if (k != null) dots.push(at(el('span', { class: 'mwp-chart__dot', 'data-series': colour(i) }), { '--_x': x(k), '--_y': y(v) }));
        }
        area.append(lines, ...dots); // after the grid and the guide, so the line paints over both
      }
      if (spark) drawing.append(area);
      else {
        const fmt = new Intl.NumberFormat(this.closest('[lang]')?.lang || undefined, { notation: 'compact', maximumSignificantDigits: digits });
        const xlabels = rows.map((row) => el('span', { class: 'mwp-chart__x' }, [text(row.cells[0])]));
        area.append(tip);
        if (this.hasAttribute('unit')) drawing.append(el('span', { class: 'mwp-chart__unit' }, [this.getAttribute('unit')]));
        drawing.append(el('div', { class: 'mwp-chart__frame' }, [
          el('div', { class: 'mwp-chart__ticks' }, steps.map((v) => at(el('span', { class: 'mwp-chart__tick' }, [fmt.format(v)]), { '--_y': y(v) }))),
          area,
          el('div', { class: 'mwp-chart__xlabels' }, xlabels),
        ]));
        this.points = n;
        this.parts = { area, guide, tip, rows, heads, colour, text, xlabels };
        // Thin again when the area changes width (the card, or the tick column) or the last label does (a font that
        // loads late, a brand with another font). Thinning never resizes either, so this cannot loop.
        this.resizer?.observe(area);
        this.resizer?.observe(xlabels.at(-1));
        this.tabIndex = 0;
        this.setAttribute('role', 'group');
        if (!this.hasAttribute('aria-label') && table.caption) { this.setAttribute('aria-label', text(table.caption)); this.named = true; }
        const pick = (e) => {
          const r = area.getBoundingClientRect();
          const f = (e.clientX - r.left) / r.width;
          this.show(Math.max(0, Math.min(n - 1, Math.floor((getComputedStyle(this).direction === 'rtl' ? 1 - f : f) * n))));
        };
        area.addEventListener('pointermove', pick);
        area.addEventListener('pointerdown', pick); // a tap picks its point before the focus event shows the active one
        area.addEventListener('pointerleave', () => { if (document.activeElement !== this) this.show(null); });
      }
    }
    this.drawing = drawing;
    table.before(drawing);
    this.thin();
    this.observer?.takeRecords();
    if (this.active >= n) this.active = null;
    if (this.points && document.activeElement === this) this.show(this.active ?? n - 1);
  }
  // Row labels by the drawn width: every k-th, counted back from the last, which always shows. k is the fewest slots
  // that hold the widest label (its padding is the gap) plus the shift that keeps an edge label inside the area.
  thin() {
    const p = this.parts;
    const n = this.points;
    const slot = p?.xlabels ? p.area.getBoundingClientRect().width / n : 0;
    if (!slot) return; // not laid out (a closed tab, a hidden panel): the resize observer calls again once it is
    p.xlabels.forEach((x, k) => { x.textContent = p.text(p.rows[k].cells[0]); delete x.dataset.edge; });
    const widths = p.xlabels.map((x) => x.getBoundingClientRect().width); // rects, not offsetWidth: it rounds, and a fraction clips
    const w = Math.max(...widths);
    const every = Math.ceil((w + Math.max(0, w - slot) / 2) / slot);
    p.xlabels.forEach((x, k) => { if ((n - 1 - k) % every) x.textContent = ''; });
    const first = (n - 1) % every;
    if ((first + 0.5) * slot < widths[first] / 2) p.xlabels[first].dataset.edge = 'start';
    if (widths[n - 1] > slot) p.xlabels[n - 1].dataset.edge = 'end';
    // k leaves room for one edge shift per gap. Only the first gap can hold two (start and end with nothing between):
    // then the first label goes, the last stays.
    const span = (k) => {
      const edge = p.xlabels[k].dataset.edge;
      const a = edge === 'start' ? k * slot : edge === 'end' ? (k + 1) * slot - widths[k] : (k + 0.5) * slot - widths[k] / 2;
      return [a, a + widths[k]];
    };
    if (first < n - 1 && span(first)[1] > span(first + every)[0]) { p.xlabels[first].textContent = ''; delete p.xlabels[first].dataset.edge; }
  }
  // The guide band and the tooltip for point i, or hide both for null. The tooltip sits beside the point, inside the area.
  show(i) {
    const p = this.parts;
    if (!p) return;
    p.guide.hidden = p.tip.hidden = i == null;
    if (i == null) return;
    this.active = i;
    p.guide.style.setProperty('--_i', i);
    const row = p.rows[i];
    p.tip.replaceChildren(el('span', {}, [p.text(row.cells[0])]), ...p.heads.map((h, s) =>
      el('span', { class: 'mwp-chart__key', 'data-series': p.colour(s) }, [el('strong', {}, [p.text(row.cells[s + 1])]), ' ', p.text(h)])));
    const w = p.area.clientWidth;
    const slot = w / this.points;
    const centre = (getComputedStyle(this).direction === 'rtl' ? this.points - i - 0.5 : i + 0.5) * slot;
    const tw = p.tip.offsetWidth;
    p.tip.style.left = `${centre + slot / 2 + tw <= w ? centre + slot / 2 : Math.max(0, centre - slot / 2 - tw)}px`;
  }
}

class ToastHost extends HTMLElement {
  connectedCallback() {
    this.setAttribute('aria-live', 'polite');
    if (!this.matches(':popover-open')) this.showPopover();
  }
}

for (const [name, ctor] of [['mwp-tabs', Tabs], ['mwp-mode-toggle', ModeToggle], ['mwp-menu', Menu], ['mwp-sort', Sort], ['mwp-chart', Chart], ['mwp-toast-host', ToastHost]])
  if (!customElements.get(name)) customElements.define(name, ctor);

if (!document.querySelector('mwp-toast-host')) document.body.append(el('mwp-toast-host', { popover: 'manual' }));

// Arrow Down/Up on a menu trigger opens its menu on the first/last item, whenever the trigger was rendered.
document.addEventListener('keydown', (e) => {
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  const menu = document.getElementById(e.target.closest?.('[popovertarget]')?.getAttribute('popovertarget') ?? '');
  if (!(menu instanceof Menu)) return;
  e.preventDefault();
  menu.focusLast = e.key === 'ArrowUp';
  menu.togglePopover(true);
});

// aria-busy blocks every activation, not only pointer clicks: Enter, Space and implicit form submission
// all dispatch a click, so one capture-phase listener covers them and focus stays where it was.
document.addEventListener('click', (e) => {
  if (!e.target.closest?.('.mwp-btn[aria-busy="true"], button[aria-busy="true"]')) return;
  e.preventDefault();
  e.stopImmediatePropagation();
}, true);

// command/commandfor and closedby="any" are the platform's; until every engine in the floor has them, two fallbacks.
document.addEventListener('click', (e) => {
  const invoker = e.target.closest?.('[command][commandfor]');
  if (invoker && !('command' in invoker)) {
    const target = document.getElementById(invoker.getAttribute('commandfor'));
    const command = invoker.getAttribute('command');
    if (command === 'show-modal') target?.showModal(); else if (command === 'close') target?.close();
  }
  if (e.target instanceof HTMLDialogElement && e.target.getAttribute('closedby') === 'any' && !('closedBy' in e.target)) e.target.close();
});

try {
  const mode = localStorage.getItem(MODE_KEY);
  if (mode && MODES.includes(mode)) html.dataset.mode = mode;
  // A saved brand only takes effect when the page links that brand's file; otherwise the attribute matches nothing and ink stays.
  const brand = localStorage.getItem(BRAND_KEY);
  if (brand) html.dataset.brand = brand;
  const palette = localStorage.getItem(PALETTE_KEY);
  if (palette) html.dataset.palette = palette;
} catch {}

globalThis.mwp = mwp;
export default mwp;
