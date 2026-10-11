//! Dragging by a handle and swiping a row away, for Today and Upcoming.
//!
//! The pointer work happens in the web view (one small script, the same for
//! mouse, pen and finger); the result comes back here as a [`Gesture`]. A
//! screen marks its root with `data-scope`, its rows with `data-ref`, the
//! handle with `data-drag`, a row that can be swiped with `data-swipe`, and its
//! drop areas with `data-zone="focus"` / `data-zone="pool"`.

use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Gesture {
    /// Dropped on a zone; `index` is the place among that zone's rows.
    Drop {
        r#ref: String,
        to: String,
        index: usize,
        from: String,
    },
    /// Swiped to the right, out of sight.
    Swipe { r#ref: String },
}

const SCRIPT: &str = r#"
const scope = await dioxus.recv();
window.tacklyGestures = window.tacklyGestures || {};
window.tacklyGestures[scope] = (message) => dioxus.send(message);
if (!window.tacklyGesturesInstalled) {
  window.tacklyGesturesInstalled = true;
  const send = (row, message) => {
    const root = row.closest('[data-scope]');
    const to = root && window.tacklyGestures[root.dataset.scope];
    if (to) to(message);
  };
  const over = (el, y) => { const r = el && el.getBoundingClientRect(); return !!r && y >= r.top && y <= r.bottom; };
  let drag = null, swipe = null;
  document.addEventListener('pointerdown', (e) => {
    const handle = e.target.closest('[data-drag]');
    if (handle) {
      const row = handle.closest('[data-ref]'), box = row.getBoundingClientRect();
      e.preventDefault();
      const ghost = row.cloneNode(true);
      ghost.removeAttribute('data-ref');
      Object.assign(ghost.style, { position: 'fixed', zIndex: 60, pointerEvents: 'none', left: box.left + 'px', top: box.top + 'px', width: box.width + 'px', margin: 0, boxShadow: '0 12px 28px rgb(0 0 0 / .25)', transform: 'rotate(1deg)' });
      document.body.append(ghost);
      row.style.opacity = '.3';
      drag = { row, ghost, dy: e.clientY - box.top, from: (row.closest('[data-zone]') || {}).dataset?.zone || '' };
      return;
    }
    const row = e.target.closest('[data-swipe]');
    // A row may be one big button (a problem); a click after a swipe is dropped below.
    if (row && !e.target.closest('input, label')) swipe = { row, x: e.clientX, y: e.clientY, dx: 0 };
  });
  document.addEventListener('pointermove', (e) => {
    if (drag) {
      drag.ghost.style.top = (e.clientY - drag.dy) + 'px';
      for (const zone of drag.row.closest('[data-scope]').querySelectorAll('[data-zone]')) {
        zone.style.outline = over(zone, e.clientY) ? '2px solid var(--color-accent-700)' : '';
      }
      return;
    }
    if (!swipe) return;
    swipe.dx = Math.max(0, e.clientX - swipe.x);
    if (Math.abs(e.clientY - swipe.y) > 24 && swipe.dx < 24) { swipe.row.style.transform = ''; swipe = null; return; }
    Object.assign(swipe.row.style, { transform: 'translateX(' + swipe.dx + 'px)', opacity: String(1 - swipe.dx / 400), transition: 'none' });
  });
  document.addEventListener('pointerup', (e) => {
    if (drag) {
      const { row, ghost, from } = drag; drag = null; ghost.remove(); row.style.opacity = '';
      const root = row.closest('[data-scope]');
      for (const zone of root.querySelectorAll('[data-zone]')) {
        zone.style.outline = '';
        if (!over(zone, e.clientY)) continue;
        const index = [...zone.querySelectorAll('[data-ref]')].filter((r) => r !== row)
          .filter((r) => { const b = r.getBoundingClientRect(); return b.top + b.height / 2 < e.clientY; }).length;
        send(row, { kind: 'drop', ref: row.dataset.ref, to: zone.dataset.zone, index, from });
      }
      return;
    }
    if (!swipe) return;
    const { row, dx } = swipe; swipe = null;
    row.style.transition = 'transform .2s, opacity .2s';
    if (dx < 110) { row.style.transform = ''; row.style.opacity = ''; return; }
    row.style.transform = 'translateX(110%)'; row.style.opacity = '0';
    // The click that ends a swipe must not open the row (any other click goes through: Undo).
    const stop = (c) => {
      window.removeEventListener('click', stop, { capture: true });
      if (row.contains(c.target)) { c.stopPropagation(); c.preventDefault(); }
    };
    window.addEventListener('click', stop, { capture: true });
    // Sent at once: a window in the background may pause its timers.
    send(row, { kind: 'swipe', ref: row.dataset.ref });
  });
}
while (true) { await dioxus.recv(); }
"#;

/// Listens for gestures in the element marked `data-scope="{scope}"`.
pub fn use_gestures(scope: &'static str, on: Callback<Gesture>) {
    use_future(move || async move {
        let mut eval = document::eval(SCRIPT);
        if eval.send(scope).is_err() {
            return;
        }
        while let Ok(gesture) = eval.recv::<Gesture>().await {
            on.call(gesture);
        }
    });
}
