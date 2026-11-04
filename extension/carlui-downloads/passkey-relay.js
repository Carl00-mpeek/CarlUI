// Passkey aktarıcısı (eklentinin yalıtılmış dünyası): passkey-page.js'nin
// isteğini arka plana iletir, yanıtı sayfaya geri verir. Yalnızca ana çerçeve.
// Arka plan adres olarak sayfanın söylediğini değil, tarayıcının bildirdiği
// sekme adresini kullanır.

(() => {
  if (window.top !== window || window.__carlPasskeyRelay) return;
  window.__carlPasskeyRelay = true;
  const TAG = "__carlui_pk";

  const send = (msg) =>
    new Promise((ok) => {
      try {
        if (globalThis.browser?.runtime) {
          browser.runtime.sendMessage(msg).then(ok, () => ok(null));
        } else {
          chrome.runtime.sendMessage(msg, (r) => ok(chrome.runtime.lastError ? null : r));
        }
      } catch {
        ok(null);
      }
    });

  window.addEventListener("message", (e) => {
    const d = e.data;
    if (e.source !== window || !d || typeof d !== "object" || d[TAG] !== "req" || (d.op !== "create" && d.op !== "get")) return;
    const id = String(d.id || "");
    window.postMessage({ [TAG]: "ack", id }, location.origin);
    send({ op: "vault-passkey", kind: d.op, data: d.data }).then((result) => window.postMessage({ [TAG]: "res", id, result }, location.origin));
  });
})();
