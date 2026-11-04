// CarlUI İndirmeler: tarayıcıda başlayan indirmeyi duraklatır, CarlUI adasına
// "İndirme bilgisi" kartı olarak sorar ve verilen karara göre iptal eder (CarlUI
// indiriyor) ya da sürdürür (tarayıcıda kalsın). CarlUI çalışmıyorsa ya da
// indirme yöneticisi kapalıysa hiçbir şeye dokunmaz; o durumda tarayıcının kendi
// indirmelerinin ilerlemesini adaya bildirir.
//
// Yalnızca bu bilgisayardaki 127.0.0.1:47321 adresine konuşur. Çerezler,
// oturum gerektiren indirmeler (ör. Google Drive) CarlUI'dan da inebilsin diye
// yalnızca o adrese ve yalnızca o indirme için gönderilir.

const API = globalThis.browser ?? globalThis.chrome;
const BASE = "http://127.0.0.1:47321";
const HDR = { "Content-Type": "application/json", "X-CarlUI": "1" };

async function call(path, body, timeoutMs) {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), timeoutMs);
  try {
    const res = await fetch(BASE + path, {
      method: body === undefined ? "GET" : "POST",
      headers: HDR,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: ctl.signal,
    });
    return res.ok ? await res.json() : null;
  } catch {
    return null;
  } finally {
    clearTimeout(t);
  }
}

API.downloads.onCreated.addListener(async (item) => {
  const url = item.finalUrl || item.url || "";
  // blob:/data: adresleri yalnızca tarayıcıda anlamlı; onlara karışma.
  if (!/^https?:/i.test(url) || item.byExtensionId || item.state !== "in_progress") return;

  const ping = await call("/ping", undefined, 800);
  if (!ping || !ping.enabled) {
    watchProgress();
    return;
  }

  let paused = false;
  try {
    await API.downloads.pause(item.id);
    paused = true;
  } catch {}

  let cookies = "";
  try {
    const list = await API.cookies.getAll({ url });
    cookies = list.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {}

  const answer = await call(
    "/offer",
    {
      url,
      filename: item.filename || "",
      size: item.totalBytes > 0 ? item.totalBytes : item.fileSize > 0 ? item.fileSize : 0,
      referrer: item.referrer || "",
      cookies,
      ua: navigator.userAgent,
    },
    250000
  );

  if (answer && answer.action === "carlui") {
    try { await API.downloads.cancel(item.id); } catch {}
    try { await API.downloads.erase({ id: item.id }); } catch {}
  } else {
    if (paused) {
      try { await API.downloads.resume(item.id); } catch {}
    }
    watchProgress();
  }
});

// Tarayıcının kendi indirmeleri: saniyede bir adaya ilerleme bildir.
let watching = false;
const last = new Map();

async function watchProgress() {
  if (watching) return;
  watching = true;
  try {
    for (;;) {
      const list = await API.downloads.search({ state: "in_progress" });
      const active = list.filter((d) => !d.paused);
      if (active.length === 0) break;
      const now = Date.now();
      const items = active.map((d) => {
        const prev = last.get(d.id);
        const speed = prev ? Math.max(0, ((d.bytesReceived - prev.got) * 1000) / Math.max(1, now - prev.at)) : 0;
        last.set(d.id, { got: d.bytesReceived, at: now });
        return { name: d.filename || d.url, got: d.bytesReceived, total: d.totalBytes > 0 ? d.totalBytes : 0, speed };
      });
      if (!(await call("/progress", { items }, 800))) break; // CarlUI kapandı
      await new Promise((r) => setTimeout(r, 1000));
    }
  } finally {
    watching = false;
    last.clear();
  }
}

API.downloads.onChanged.addListener((d) => {
  if (d.state || d.paused) watchProgress();
});

// Sayfa içi "Carl Vault" düğmesi (content.js). Adres sayfanın söylediği değil,
// tarayıcının bildirdiği sekme adresidir; yalnızca ana çerçeve.
//
// Gönderilen giriş/kayıt formu (content.js "vault-capture"): kullanıcı adı ve
// parola yalnızca burada, bellekte ve sekme başına en çok 3 dk tutulur; sayfa
// geri almaz. Kasa "yeni" ya da "parola değişmiş" derse sayfa (form sonrası
// açılan sayfa da) "Kaydedilsin mi?" sorar; kasaya ancak "Kaydet"e basılınca
// yazılır. Aynısı kasada varsa hiçbir şey sorulmaz.
const PENDING_MS = 180000;
const pending = new Map(); // sekme → { url, user, password, signup, at, check }

function forget(tab) {
  const p = pending.get(tab);
  if (p) p.password = p.user = "";
  pending.delete(tab);
}

function capture(tab, url, msg) {
  const user = String(msg.user || "").slice(0, 1024);
  const password = String(msg.password || "").slice(0, 1024);
  if (!password) return Promise.resolve({ show: false });
  const old = pending.get(tab);
  if (old && old.url === url && old.user === user && old.password === password) return old.check;
  forget(tab);
  const p = { url, user, password, signup: !!msg.signup, at: Date.now() };
  p.check = call("/vault/save", { url, user, password, commit: false }, 5000).then((r) => {
    // Kasa kapalı/kilitli: durumu bilinemez; yalnızca kayıt formunda sorulur.
    const status = !r ? "closed" : r.locked ? "locked" : r.ok ? r.status : "";
    if (r?.off || status === "same" || !status || ((status === "closed" || status === "locked") && !p.signup)) {
      if (pending.get(tab) === p) forget(tab);
      return { show: false };
    }
    p.status = status;
    return info(p);
  });
  pending.set(tab, p);
  setTimeout(() => pending.get(tab) === p && forget(tab), PENDING_MS);
  return p.check;
}

// Sayfaya giden bilgi: parola yok.
function info(p) {
  let site = "";
  try {
    site = new URL(p.url).hostname.replace(/^www\./, "");
  } catch {}
  return { show: true, status: p.status, site, user: p.user };
}

async function save(tab) {
  const p = pending.get(tab);
  if (!p || Date.now() - p.at > PENDING_MS) return { ok: false, gone: true, message: "Süre doldu." };
  const r = await call("/vault/save", { url: p.url, user: p.user, password: p.password, commit: true }, 5000);
  // Kasa kilitli/öne gelmiş ya da kapalı: bilgi kilit açılana dek bekler.
  if (!r || r.locked || (!r.ok && !r.status)) return r ? { ...r, retry: true } : { ok: false, closed: true, retry: true };
  forget(tab);
  return r;
}

API.tabs?.onRemoved?.addListener((tab) => forget(tab));

API.runtime.onMessage.addListener((msg, sender, reply) => {
  if (!sender.tab || sender.frameId !== 0 || !/^https?:/i.test(sender.url || "")) return false;
  const url = sender.url;
  const tab = sender.tab.id;
  const run =
    msg.op === "vault-find" ? call("/vault/find", { url }, 5000)
    : msg.op === "vault-fill" ? call("/vault/fill", { url, id: String(msg.id || "") }, 5000)
    : msg.op === "vault-open" ? call("/vault/open", {}, 15000)
    : msg.op === "vault-capture" ? capture(tab, url, msg)
    : msg.op === "vault-pending" ? (pending.get(tab)?.check ?? Promise.resolve({ show: false }))
    : msg.op === "vault-save" ? save(tab)
    : msg.op === "vault-dismiss" ? Promise.resolve((forget(tab), { ok: true }))
    : null;
  if (!run) return false;
  run.then((r) => reply(r ?? { ok: false, closed: true, message: "CarlUI çalışmıyor." }));
  return true;
});
