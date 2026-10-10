// Playwright-style engine, injected into the app's web view.
//
// Locators are plain data (a chain of steps) resolved fresh on every use, like
// Playwright's. Actions run the same actionability checks: exactly one match
// (strict mode), visible, enabled, stable, and receiving pointer events at its
// centre. A failed check returns {status:"retry"}; the Rust side keeps trying
// until its timeout, like Playwright's auto-waiting.
window.pw = (() => {
  const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
  // No timers in here: a window that is not in front gets its timers paused,
  // so all waiting happens on the Rust side, between calls.

  // ---- text, roles, accessible names --------------------------------------
  const textOf = (node) => {
    if (node.nodeType === 3) return node.nodeValue;
    if (node.nodeType !== 1 || node.getAttribute("aria-hidden") === "true") return "";
    if (["SCRIPT", "STYLE"].includes(node.tagName)) return "";
    return [...node.childNodes].map(textOf).join(" ");
  };
  const roleOf = (el) => {
    const explicit = el.getAttribute("role");
    if (explicit) return explicit;
    const tag = el.tagName;
    if (tag === "BUTTON") return "button";
    if (tag === "A" && el.hasAttribute("href")) return "link";
    if (tag === "TEXTAREA") return "textbox";
    if (tag === "INPUT") return ["checkbox", "radio"].includes(el.type) ? el.type : "textbox";
    if (/^H[1-6]$/.test(tag)) return "heading";
    if (tag === "IMG") return "img";
    return "";
  };
  const labelTexts = (el) => [...(el.labels || [])].map((l) => norm(textOf(l)));
  const nameOf = (el) => {
    const aria = el.getAttribute("aria-label");
    if (aria) return norm(aria);
    const by = el.getAttribute("aria-labelledby");
    if (by) return norm(by.split(" ").map((id) => textOf(document.getElementById(id) || document.body)).join(" "));
    const labels = labelTexts(el);
    if (labels.length) return labels.join(" ");
    if (["button", "heading", "link"].includes(roleOf(el))) return norm(textOf(el));
    return norm(el.getAttribute("placeholder") || el.getAttribute("title") || "");
  };
  const matches = (actual, wanted, exact) =>
    exact ? norm(actual) === norm(wanted) : norm(actual).toLowerCase().includes(norm(wanted).toLowerCase());

  // ---- state ---------------------------------------------------------------
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.display !== "none";
  };
  const enabled = (el) => !el.disabled && el.getAttribute("aria-disabled") !== "true";
  const editable = (el) =>
    ["INPUT", "TEXTAREA"].includes(el.tagName) && !el.readOnly && !el.disabled;
  const describe = (el) =>
    `<${el.tagName.toLowerCase()}${el.className ? ` class="${el.className}"` : ""}> ${norm(textOf(el)).slice(0, 40)}`;

  // ---- locator resolution ----------------------------------------------------
  const within = (roots, pick) => {
    const seen = new Set();
    const out = [];
    for (const root of roots) {
      for (const el of pick(root)) {
        if (!seen.has(el)) {
          seen.add(el);
          out.push(el);
        }
      }
    }
    return out;
  };
  const all = (root) => [...(root.querySelectorAll ? root.querySelectorAll("*") : [])];
  const steps = {
    css: (roots, s) => within(roots, (r) => [...r.querySelectorAll(s.v)]),
    role: (roots, s) =>
      within(roots, (r) =>
        all(r).filter((el) => roleOf(el) === s.role && (s.name === undefined || matches(nameOf(el), s.name, s.exact))),
      ),
    label: (roots, s) =>
      within(roots, (r) =>
        all(r).filter((el) => [...labelTexts(el), norm(el.getAttribute("aria-label"))].some((t) => t && matches(t, s.v, s.exact))),
      ),
    text: (roots, s) =>
      within(roots, (r) => {
        const hit = (el) => matches(textOf(el), s.v, s.exact);
        // the deepest elements that contain the text
        return all(r).filter((el) => hit(el) && ![...el.children].some(hit));
      }),
    filter: (list, s) =>
      list.filter(
        (el) =>
          (s.hasText === undefined || matches(textOf(el), s.hasText, false)) &&
          (s.hasNotText === undefined || !matches(textOf(el), s.hasNotText, false)),
      ),
    nth: (list, s) => {
      const el = s.n < 0 ? list[list.length + s.n] : list[s.n];
      return el ? [el] : [];
    },
  };
  const resolve = (chain) => {
    let list = [document];
    for (const step of chain) {
      const run = steps[step.k];
      if (!run) throw new Error("unknown locator step " + step.k);
      list = run(list, step);
    }
    return list;
  };

  // ---- actions -----------------------------------------------------------------
  const setValue = (el, value) => {
    const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
  };
  const keyEvent = (el, type, key, extra = {}) => {
    const e = new KeyboardEvent(type, { key, code: key.length === 1 ? "Key" + key.toUpperCase() : key, bubbles: true, cancelable: true, ...extra });
    el.dispatchEvent(e);
    return e;
  };
  const inputEvent = (el, inputType, data) =>
    el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType, data }));

  // Playwright's "stable": the same box on two consecutive attempts.
  const lastBox = new Map();
  const clickable = async (el, key) => {
    el.scrollIntoView({ block: "center" });
    const b = el.getBoundingClientRect();
    const box = [b.x, b.y, b.width, b.height].join();
    const before = lastBox.get(key);
    lastBox.set(key, box);
    if (before !== box) return { why: "element is not stable" };
    const x = b.left + b.width / 2;
    const y = b.top + b.height / 2;
    const hit = document.elementFromPoint(x, y);
    if (!hit || !el.contains(hit)) {
      return { why: `${hit ? describe(hit) : "nothing"} intercepts pointer events` };
    }
    return { hit, x, y };
  };
  const pointerClick = (hit, x, y) => {
    const init = { bubbles: true, cancelable: true, view: window, clientX: x, clientY: y, button: 0 };
    for (const type of ["pointerover", "mouseover", "pointermove", "mousemove", "pointerdown", "mousedown"]) {
      hit.dispatchEvent(new MouseEvent(type, init));
    }
    const focusable = hit.closest("button, input, textarea, a[href], [tabindex]");
    if (focusable) focusable.focus({ preventScroll: true });
    hit.dispatchEvent(new MouseEvent("pointerup", init));
    hit.dispatchEvent(new MouseEvent("mouseup", init));
    // SVG elements (a drawing inside a button) have no click(); a click event
    // still bubbles to the button like a real tap.
    if (typeof hit.click === "function") hit.click();
    else hit.dispatchEvent(new MouseEvent("click", init));
  };

  const ops = {
    click: async (el, arg, key) => {
      const ready = await clickable(el, key);
      if (ready.why) return ready;
      pointerClick(ready.hit, ready.x, ready.y);
      return { done: true };
    },
    check: async (el, arg, key) => {
      if (el.checked) return { done: true };
      return ops.click(el, arg, key);
    },
    fill: async (el, text) => {
      if (!editable(el)) return { why: "element is not editable" };
      el.focus();
      el.select();
      setValue(el, text);
      inputEvent(el, "insertReplacementText", text);
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return { done: true };
    },
    // One key: keydown, keypress, beforeinput, insert at the caret, input, keyup.
    // The Rust side sends them one call at a time, with the delay in between.
    type_key: async (el, ch) => {
      if (!editable(el)) return { why: "element is not editable" };
      if (document.activeElement !== el) el.focus();
      keyEvent(el, "keydown", ch);
      const press = keyEvent(el, "keypress", ch);
      const before = new InputEvent("beforeinput", { bubbles: true, cancelable: true, inputType: "insertText", data: ch });
      el.dispatchEvent(before);
      if (!press.defaultPrevented && !before.defaultPrevented) {
        const start = el.selectionStart ?? el.value.length;
        const end = el.selectionEnd ?? el.value.length;
        el.setRangeText(ch, start, end, "end");
        inputEvent(el, "insertText", ch);
      }
      keyEvent(el, "keyup", ch);
      return { done: true };
    },
    press: async (el, key) => {
      el.focus();
      const combo = key.split("+");
      const name = combo.pop();
      if (combo.length && name.toLowerCase() === "a" && editable(el)) {
        el.select();
        return { done: true };
      }
      const down = keyEvent(el, "keydown", name);
      if (name === "Backspace" && editable(el) && !down.defaultPrevented) {
        const start = el.selectionStart ?? el.value.length;
        const end = el.selectionEnd ?? el.value.length;
        if (start !== end) el.setRangeText("", start, end, "end");
        else if (start > 0) el.setRangeText("", start - 1, start, "end");
        inputEvent(el, "deleteContentBackward", null);
      }
      if (name === "Enter") {
        keyEvent(el, "keypress", name);
        if (el.tagName === "BUTTON" && !down.defaultPrevented) el.click();
      }
      keyEvent(el, "keyup", name);
      return { done: true };
    },
    input_value: async (el) => ({ done: true, value: el.value }),
    text_content: async (el) => ({ done: true, value: norm(textOf(el)) }),
    // Reads the QR code the way a camera would: draw the SVG, return grey pixels.
    qr_pixels: async (el) => {
      const svg = el.tagName.toLowerCase() === "svg" ? el : el.querySelector("svg");
      if (!svg) return { why: "no QR code drawn yet" };
      const image = new Image();
      image.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(new XMLSerializer().serializeToString(svg));
      await image.decode();
      const size = 640;
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = size;
      const g = canvas.getContext("2d");
      g.fillStyle = "#fff";
      g.fillRect(0, 0, size, size);
      g.drawImage(image, 0, 0, size, size);
      const rgba = g.getImageData(0, 0, size, size).data;
      let grey = "";
      for (let i = 0; i < rgba.length; i += 4) {
        grey += String.fromCharCode(Math.round(0.299 * rgba[i] + 0.587 * rgba[i + 1] + 0.114 * rgba[i + 2]));
      }
      return { done: true, value: { size, grey: btoa(grey) } };
    },
  };
  // actions that need the element to be shown and enabled first
  const needsEnabled = new Set(["click", "check", "fill", "type_key", "press"]);
  const needsVisible = new Set(["click", "check", "fill", "type_key", "press", "input_value", "text_content"]);

  return {
    // One attempt of an action; the caller retries on {status:"retry"}.
    act: async (chain, op, arg, desc) => {
      const found = resolve(chain);
      if (found.length === 0) return { status: "retry", reason: "waiting for " + desc };
      if (found.length > 1) {
        throw new Error(
          `strict mode violation: ${desc} resolved to ${found.length} elements:\n` +
            found.slice(0, 5).map((el) => "    " + describe(el)).join("\n"),
        );
      }
      const el = found[0];
      if (needsVisible.has(op) && !visible(el)) return { status: "retry", reason: "element is not visible" };
      if (needsEnabled.has(op) && !enabled(el)) return { status: "retry", reason: "element is not enabled" };
      const result = await ops[op](el, arg, desc);
      if (result.why) return { status: "retry", reason: result.why };
      return { status: "done", value: result.value ?? null };
    },
    // What `expect` looks at.
    query: (chain) => {
      const found = resolve(chain);
      return {
        count: found.length,
        items: found.slice(0, 5).map((el) => ({
          visible: visible(el),
          enabled: enabled(el),
          text: norm(textOf(el)),
          value: "value" in el ? el.value : null,
          checked: !!el.checked,
          html: describe(el),
        })),
      };
    },
    pageText: () => norm(textOf(document.body)),
  };
})();
return true;
