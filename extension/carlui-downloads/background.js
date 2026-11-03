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
