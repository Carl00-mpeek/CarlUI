// Carl Vault ile doldurma. Kullanıcı bu pencerede bir kayda tıklayınca giriş
// bilgileri CarlUI'dan (127.0.0.1:47321 → kasa) istenir ve yalnızca bu sekmenin
// ana çerçevesine, sayfa kökeni hâlâ aynıysa yazılır. Kasa yalnızca açıkken,
// kayıttaki adres bu sayfaya uyuyorsa ve tarayıcı öndeyken yanıt verir.
// Eklenti parolayı saklamaz; yalnızca doldururken bellekte tutar.

const API = globalThis.browser ?? globalThis.chrome;
const BASE = "http://127.0.0.1:47321";
const HDR = { "Content-Type": "application/json", "X-CarlUI": "1" };
const $ = (id) => document.getElementById(id);

// Arayüz dili: CarlUI'de seçilen dil (/ping), CarlUI kapalıysa tarayıcının dili.
let lang = /^tr\b/i.test(navigator.language || "") ? "tr" : "en";
const L = (tr, en) => (lang === "tr" ? tr : en);

async function raw(path, body, get, token) {
  const headers = token ? { ...HDR, "X-CarlUI": token } : HDR;
  try {
    const res = await fetch(BASE + path, get ? { headers } : { method: "POST", headers, body: JSON.stringify(body ?? {}) });
    return res.ok ? await res.json() : null;
  } catch {
    return null;
  }
}

// Eşleştirme belirteci (bkz. background.js): kasa istekleri onaylı eklentiden gelmeli.
async function savedToken() {
  try {
    return (await API.storage.local.get("carluiToken")).carluiToken || "";
  } catch {
    return "";
  }
}

async function call(path, body, get) {
  if (!path.startsWith("/vault/")) return raw(path, body, get, "");
  let r = await raw(path, body, get, await savedToken());
  if (r && r.pair) {
    note(L("CarlUI'da bağlantıyı onayla…", "Approve the connection in CarlUI…"));
    const p = await raw("/vault/pair", {}, false, "");
    if (p && p.token) {
      await API.storage.local.set({ carluiToken: p.token });
      r = await raw(path, body, get, p.token);
    } else if (p && p.message) {
      r = { ok: false, message: p.message };
    }
  }
  return r;
}

function note(text, warn) {
  $("note").textContent = text || "";
  $("note").className = warn ? "warn" : "";
}

function button(cls, html, onclick) {
  const b = document.createElement("button");
  b.className = cls;
  b.append(...html);
  b.addEventListener("click", onclick);
  $("list").append(b);
  return b;
}

function el(tag, text) {
  const e = document.createElement(tag);
  e.textContent = text;
  return e;
}

// Sayfada çalışır: kullanıcı adı ve parola kutularını doldurur.
function fillPage(user, pass, origin) {
  if (location.origin !== origin) return { ok: false, why: "origin" };
  const visible = (e) => !e.disabled && !e.readOnly && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden";
  const set = (e, v) => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    e.focus();
    setter.call(e, v);
    e.dispatchEvent(new Event("input", { bubbles: true }));
    e.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const inputs = [...document.querySelectorAll("input")].filter(visible);
  const pw = inputs.find((e) => e.type === "password");
  const userLike = (e) =>
    ["text", "email", "tel", ""].includes(e.type) &&
    /user|email|e-posta|login|kullan|account|hesap|ad$/i.test([e.name, e.id, e.autocomplete, e.placeholder, e.getAttribute("aria-label")].join(" "));
  let u = null;
  if (pw) {
    // Parola kutusundan önceki en yakın metin kutusu (aynı formda olanı tercih et).
    const before = inputs.slice(0, inputs.indexOf(pw)).filter((e) => ["text", "email", "tel", ""].includes(e.type));
    u = before.filter((e) => e.form === pw.form).pop() || before.pop() || null;
  } else {
    // Önce kullanıcı adı soran sayfalar (ör. Google).
    u = inputs.find((e) => e.autocomplete === "username") || inputs.find(userLike) || null;
  }
  let n = 0;
  if (u && user) { set(u, user); n++; }
  if (pw && pass) { set(pw, pass); n++; }
  return { ok: n > 0, filled: n, password: !!pw };
}

async function fill(tab, origin, login) {
  note(L("Dolduruluyor…", "Filling…"));
  const r = await call("/vault/fill", { url: tab.url, id: login.id });
  if (!r || !r.ok || !r.fill) {
    note((r && r.message) || L("CarlUI'ya ulaşılamadı.", "Couldn't reach CarlUI."), true);
    return;
  }
  let [user, pass] = r.fill;
  r.fill = null;
  try {
    const [res] = await API.scripting.executeScript({ target: { tabId: tab.id, frameIds: [0] }, func: fillPage, args: [user, pass, origin] });
    const out = res && res.result;
    if (out && out.ok) {
      window.close();
      return;
    }
    note(out && out.why === "origin" ? L("Sayfa değişti; tekrar dene.", "The page changed; try again.") : L("Bu sayfada giriş kutusu bulunamadı.", "No sign-in box found on this page."), true);
  } catch (e) {
    note(L("Bu sayfaya yazılamadı.", "Couldn't write to this page."), true);
  } finally {
    user = pass = "";
  }
}

async function main() {
  const p = await call("/ping", undefined, true);
  if (p && (p.lang === "tr" || p.lang === "en")) lang = p.lang;
  document.documentElement.lang = lang;
  const [tab] = await API.tabs.query({ active: true, currentWindow: true });
  let url;
  try { url = new URL(tab.url); } catch {}
  if (!url || !/^https?:$/.test(url.protocol)) {
    note(L("Bu sayfada doldurma yapılamaz.", "Filling isn't possible on this page."));
    return;
  }
  $("host").textContent = url.hostname.replace(/^www\./, "");
  const r = await call("/vault/find", { url: tab.url });
  if (!r) {
    note(L("CarlUI çalışmıyor.", "CarlUI isn't running."), true);
    return;
  }
  if (r.closed || r.locked) {
    note(r.locked ? L("Kasa kilitli. Açıp tekrar dene.", "The vault is locked. Unlock it and try again.") : L("Carl Vault açık değil.", "Carl Vault isn't open."), true);
    button("act", [el("span", L("Carl Vault'u aç", "Open Carl Vault"))], async () => {
      await call("/vault/open");
      window.close();
    });
    return;
  }
  if (!r.ok) {
    note(r.message, true);
    return;
  }
  const logins = r.logins || [];
  if (logins.length === 0) {
    note(r.message || L("Bu site için kayıt yok. Kasadaki kayda bu sitenin adresini ekle.", "No item for this site. Add this site's address to an item in the vault."));
    return;
  }
  for (const l of logins) {
    const b = button("item", [el("b", l.name), el("span", l.username || L("kullanıcı adı yok", "no username"))], () => fill(tab, url.origin, l));
    if (logins.length === 1) b.focus();
  }
  note(L("Seçince kullanıcı adı ve parola bu sayfaya yazılır.", "Pick one to fill its username and password on this page."));
}

main();
