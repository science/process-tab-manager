// Stale-safe DOM queries via browser.execute()
// All reads go through the browser context to avoid stale element references.

export async function getRowCount() {
  return browser.execute(() => document.querySelectorAll(".row").length);
}

export async function getGroupCount() {
  return browser.execute(() => document.querySelectorAll(".group-header").length);
}

export async function getRowTexts() {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".row .title"), el => el.textContent)
  );
}

export async function getRowAt(index) {
  return browser.execute((i) => {
    const row = document.querySelectorAll(".row")[i];
    if (!row) return null;
    return {
      wid: Number(row.dataset.wid),
      title: row.querySelector(".title")?.textContent || "",
      classes: row.className,
    };
  }, index);
}

export async function getRowByWid(wid) {
  return browser.execute((w) => {
    const rows = document.querySelectorAll(".row");
    for (let i = 0; i < rows.length; i++) {
      if (Number(rows[i].dataset.wid) === w) {
        return {
          wid: w,
          title: rows[i].querySelector(".title")?.textContent || "",
          classes: rows[i].className,
          index: i,
        };
      }
    }
    return null;
  }, wid);
}

export async function getGroupAt(index) {
  return browser.execute((i) => {
    const g = document.querySelectorAll(".group-header")[i];
    if (!g) return null;
    return {
      gid: g.dataset.gid,
      name: g.querySelector(".group-name")?.textContent || "",
      classes: g.className,
    };
  }, index);
}

export async function hasClass(selector, cls) {
  return browser.execute((sel, c) => {
    const el = document.querySelector(sel);
    return el ? el.classList.contains(c) : false;
  }, selector, cls);
}

export async function getBodyBgColor() {
  return browser.execute(() => getComputedStyle(document.body).backgroundColor);
}

export async function getElementText(selector) {
  return browser.execute((sel) => {
    const el = document.querySelector(sel);
    return el ? el.textContent : null;
  }, selector);
}

export async function getElementCount(selector) {
  return browser.execute((sel) => document.querySelectorAll(sel).length, selector);
}

export async function getSelectedRow() {
  return browser.execute(() => {
    const el = document.querySelector(".row.selected");
    if (!el) return null;
    return {
      wid: Number(el.dataset.wid),
      title: el.querySelector(".title")?.textContent || "",
    };
  });
}

export async function hasRenameInput() {
  return browser.execute(() => !!document.querySelector(".rename-input"));
}

export async function getRenameInputValue() {
  return browser.execute(() => {
    const input = document.querySelector(".rename-input");
    return input ? input.value : null;
  });
}
