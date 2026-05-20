/**
 * Info-grid — shared rendering for any read-only key/value surface
 * (the diagnostics card, the settings card, etc.).
 *
 * Sections (`{heading, rows}`) are rendered as an uppercase heading
 * followed by `<div class="info-row">` rows. Each row gets a label
 * cell, a value cell (either `value: string` for monospace or
 * `html: string` for richer rendering — caller is responsible for
 * escaping when using html), and an optional trailing action button.
 *
 * Cards reuse the `.info-grid`, `.info-row`, `.info-row__*` CSS
 * classes defined in index.html.
 */

/**
 * Format a byte count as a human-readable string (B / KB / MB / GB).
 * Returns "—" when the source value is null/undefined. Uses 1024-based
 * units, the file-manager convention.
 */
export function humanBytes(n) {
  if (n == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = Number(n);
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  const precision = v < 10 && i > 0 ? 1 : 0;
  return `${v.toFixed(precision)} ${units[i]}`;
}

/**
 * Build a single row element.
 *
 * @param {object} row
 * @param {string} row.label
 * @param {string} [row.value] — monospace value when no html given
 * @param {string} [row.html] — full inner HTML for the value cell
 * @param {boolean} [row.plainValue] — render text in normal (non-mono) font
 * @param {object} [row.action] — { label, onClick, danger? } — trailing button
 */
export function renderRow(row) {
  const el = document.createElement("div");
  el.className = "info-row";

  const labelEl = document.createElement("div");
  labelEl.className = "info-row__label";
  labelEl.textContent = row.label;
  el.appendChild(labelEl);

  const valueWrap = document.createElement("div");
  valueWrap.className = "info-row__valuewrap";
  if (row.html) {
    valueWrap.innerHTML = row.html;
  } else {
    const v = document.createElement("div");
    v.className = row.plainValue
      ? "info-row__value info-row__value--plain"
      : "info-row__value";
    v.textContent = row.value ?? "—";
    valueWrap.appendChild(v);
  }
  el.appendChild(valueWrap);

  if (row.action) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "info-row__action";
    if (row.action.danger) btn.classList.add("danger");
    btn.textContent = row.action.label;
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      try {
        await row.action.onClick();
      } finally {
        if (btn.isConnected) btn.disabled = false;
      }
    });
    el.appendChild(btn);
  }

  return el;
}

/**
 * Replace the body with the rendered sections. Sets aria-busy off
 * (assumes caller set it on before fetching data).
 *
 * @param {HTMLElement} body
 * @param {Array<{heading: string, rows: object[]}>} sections
 */
export function renderSections(body, sections) {
  body.replaceChildren();
  body.removeAttribute("aria-busy");

  for (const sec of sections) {
    const heading = document.createElement("h3");
    heading.className = "info-grid__heading";
    heading.textContent = sec.heading;
    body.appendChild(heading);
    for (const row of sec.rows) body.appendChild(renderRow(row));
  }
}

/**
 * Replace body with a centered loading message. Sets aria-busy=true.
 */
export function renderLoading(body, message) {
  if (!body) return;
  body.setAttribute("aria-busy", "true");
  body.replaceChildren();
  const div = document.createElement("div");
  div.className = "info-grid__loading";
  div.textContent = message ?? "Loading…";
  body.appendChild(div);
}

/**
 * Replace body with an error block.
 */
export function renderError(body, message) {
  if (!body) return;
  body.removeAttribute("aria-busy");
  body.replaceChildren();
  const div = document.createElement("div");
  div.className = "info-grid__error";
  div.textContent = message;
  body.appendChild(div);
}
