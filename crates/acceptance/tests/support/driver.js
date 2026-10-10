// Runs inside the app's web view. Finds elements by what a person sees and
// clicks them the way a pointer does: the element must be visible and not
// covered by anything else at its centre.
window.T = (() => {
  const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.display !== "none";
  };
  // exact text first, then ending with it (nav labels have an icon), then containing it
  const pick = (els, text, of) => {
    const label = (el) => norm(of(el));
    const shown = els.filter(visible);
    return (
      shown.find((el) => label(el) === text) ||
      shown.find((el) => label(el).endsWith(text)) ||
      shown.find((el) => label(el).includes(text)) ||
      null
    );
  };
  const kinds = {
    button: (root, t) => pick([...root.querySelectorAll("button")], t, (e) => e.innerText),
    field: (root, t) => {
      const field = pick([...root.querySelectorAll(".field")], t, (e) => e.querySelector("label").innerText);
      return field && field.querySelector("input, textarea");
    },
    card: (root, t) =>
      pick([...root.querySelectorAll(".card")], t, (e) => (e.querySelector(".title") || e).innerText),
    css: (root, t) => [...root.querySelectorAll(t)].find(visible) || null,
  };
  // "card:Do the dishes > button:Start"
  const find = (spec) => {
    let root = document;
    for (const part of spec.split(" > ")) {
      const i = part.indexOf(":");
      const kind = kinds[part.slice(0, i)];
      if (!kind) throw new Error("unknown selector kind in " + spec);
      root = kind(root, part.slice(i + 1));
      if (!root) return null;
    }
    return root;
  };
  const need = (spec) => {
    const el = find(spec);
    if (!el) throw new Error("not found: " + spec);
    return el;
  };
  return {
    find,
    has: (spec) => !!find(spec),
    text: () => norm(document.body.innerText),
    value: (spec) => need(spec).value,
    textOf: (spec) => norm(need(spec).innerText),
    cardText: (title) => {
      const el = find("card:" + title);
      return el ? norm(el.innerText) : null;
    },
    click: (spec) => {
      const el = need(spec);
      if (el.disabled) throw new Error(`"${spec}" is disabled`);
      el.scrollIntoView({ block: "center" });
      const r = el.getBoundingClientRect();
      const x = r.left + r.width / 2;
      const y = r.top + r.height / 2;
      const hit = document.elementFromPoint(x, y);
      if (!hit || !el.contains(hit)) {
        const what = hit ? `<${hit.tagName.toLowerCase()} class="${hit.className}">` : "nothing";
        throw new Error(`"${spec}" is covered by ${what}`);
      }
      const init = { bubbles: true, cancelable: true, view: window, clientX: x, clientY: y, button: 0 };
      hit.dispatchEvent(new MouseEvent("mousedown", init));
      hit.dispatchEvent(new MouseEvent("mouseup", init));
      hit.click();
      return true;
    },
    type: (spec, text) => {
      const el = need(spec);
      el.focus();
      const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, text);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      return true;
    },
  };
})();
return true;
