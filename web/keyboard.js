const FIELD = 'textarea, [contenteditable], input:not([type]), input[type="text"], input[type="search"], input[type="email"], input[type="url"], input[type="tel"], input[type="number"], input[type="password"]';
const PRIMARY = "button.primary, .btn.primary";
const ACTION = "button, .btn";
const MARGIN = 12;
const FOLLOWING = 4;

export const isTextField = (element) => Boolean(element?.matches?.(FIELD));

export const hasTextField = (html) => /<textarea\b|<input\b(?![^>]*\btype="(?:checkbox|radio|hidden|button|submit)")/.test(html);

// How far to scroll so the span's bottom clears the band's bottom, never so far that its top leaves the band; a span above the band comes back down to its top.
export function revealDelta(span, band) {
  if (span.bottom > band.bottom) return Math.max(0, Math.min(span.bottom - band.bottom, span.top - band.top));
  if (span.top < band.top) return span.top - band.top;
  return 0;
}

// The field's action is the first laid-out primary button after it in the nearest box holding one, else any button; a fixed or hidden button has no offsetParent and is lifted by CSS instead.
export function fieldAction(field, stop) {
  for (const selector of [PRIMARY, ACTION]) {
    for (let box = field.parentElement; box && box !== stop; box = box.parentElement) {
      const after = [...box.querySelectorAll(selector)].find((one) => one.offsetParent !== null && field.compareDocumentPosition(one) & FOLLOWING);
      if (after) return after;
    }
  }
  return null;
}

function scroller(field, win) {
  for (let box = field.parentElement; box && box !== field.ownerDocument.body; box = box.parentElement) {
    if (/(auto|scroll)/.test(win.getComputedStyle(box).overflowY) && box.scrollHeight > box.clientHeight) return box;
  }
  return null;
}

export function keepInView(field, view, win) {
  const pane = scroller(field, win);
  const action = fieldAction(field, pane ?? field.ownerDocument.body);
  const own = field.getBoundingClientRect();
  const span = { top: own.top - MARGIN, bottom: Math.max(own.bottom, action?.getBoundingClientRect().bottom ?? own.bottom) + MARGIN };
  let band = { top: view.offsetTop, bottom: view.offsetTop + view.height };
  if (pane) {
    const box = pane.getBoundingClientRect();
    band = { top: Math.max(band.top, box.top), bottom: Math.min(band.bottom, box.bottom) };
  }
  const delta = revealDelta(span, band);
  if (delta) (pane ?? win).scrollBy(0, delta);
  return delta;
}

// The Lead composer is fixed to the keyboard by its own rules, so only fields in the page's flow are scrolled.
export function keepFieldsAboveKeyboard(root, view = globalThis.visualViewport, page = globalThis.document?.documentElement, win = globalThis.window) {
  if (!root || !view || !page || !win) return;
  const typed = () => {
    const field = root.ownerDocument.activeElement;
    return isTextField(field) && root.contains(field) && !field.closest(".composer") ? field : null;
  };
  const check = () => {
    const field = typed();
    if (field && page.classList.contains("kb-open")) keepInView(field, view, win);
  };
  root.addEventListener("focusin", () => typed() && win.setTimeout(check, 350));
  root.addEventListener("input", () => win.requestAnimationFrame(check));
  view.addEventListener("resize", () => win.requestAnimationFrame(check));
}
