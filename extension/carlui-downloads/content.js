// Sayfa içi "Carl Vault" düğmesi. Sayfada giriş kutusu varsa ve kasada bu
// siteye uyan kayıt varsa (kasa kilitliyken bilinemez, o zaman da) kutunun
// sağına küçük bir düğme konur. Basınca:
// - Kasa açıksa kayıt (birden çoksa seçilen) kullanıcı adı ve parolayla yazılır.
// - Kasa kilitli ya da kapalıysa Carl Vault penceresi öne gelir; ana parola
//   oraya yazılır, bu sayfaya değil (sayfa kendi kutularındaki tuşları
//   okuyabilir). Kilit açılınca odak tarayıcıya döner ve doldurma sürer.
// Tüm istekler arka plandan gider; adres olarak tarayıcının bildirdiği sekme
// adresi kullanılır. Düğme kapalı gölge DOM'dadır, sayfa içine erişemez.

(() => {
  if (window.top !== window || window.__carlVault) return;
  window.__carlVault = true;

  // Firefox `browser` (söz döndürür), Chromium `chrome` (geri çağırma).
  const ask = (msg) =>
    new Promise((ok) => {
      try {
        if (globalThis.browser?.runtime) {
          browser.runtime.sendMessage(msg).then(ok, () => ok(null));
        } else {
          chrome.runtime.sendMessage(msg, (r) => ok(chrome.runtime.lastError ? null : r));
        }
      } catch {
        ok(null); // eklenti güncellendi/kapandı
      }
    });

  const visible = (e) => !e.disabled && !e.readOnly && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden" && e.offsetWidth > 40;
  const textLike = (e) => ["text", "email", "tel", ""].includes(e.type);
  const userLike = (e) =>
    textLike(e) && (e.autocomplete === "username" || /user|email|e-posta|login|kullan|account|hesap|ad$/i.test([e.name, e.id, e.autocomplete, e.placeholder, e.getAttribute("aria-label")].join(" ")));

  // Giriş kutuları: parola kutusu ve ondan önceki metin kutusu; parola yoksa
  // önce kullanıcı adı soran sayfa (ör. Google).
  function fields() {
    const inputs = [...document.querySelectorAll("input")].filter(visible);
    const pw = inputs.find((e) => e.type === "password");
    let user = null;
    if (pw) {
      const before = inputs.slice(0, inputs.indexOf(pw)).filter(textLike);
      user = before.filter((e) => e.form === pw.form).pop() || before.pop() || null;
    } else {
      user = inputs.find((e) => e.autocomplete === "username") || inputs.find(userLike) || null;
      if (user && !user.form && !/user|login|email|giri/i.test(location.pathname + document.title)) user = null;
    }
    return { user, pw, anchor: user || pw };
  }

  function setValue(e, v) {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    e.focus();
    setter.call(e, v);
    e.dispatchEvent(new Event("input", { bubbles: true }));
    e.dispatchEvent(new Event("change", { bubbles: true }));
  }

  // --- Düğme -------------------------------------------------------------
  const host = document.createElement("carl-vault");
  host.style.cssText = "all:initial;position:absolute;z-index:2147483647;top:0;left:0;display:none";
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `
<style>
  * { box-sizing: border-box; font: 600 12px/1 "Segoe UI", system-ui, sans-serif; }
  .pill { display: flex; align-items: center; gap: 5px; height: 24px; padding: 0 9px 0 6px; border: 0; border-radius: 12px;
    background: #b06ce6; color: #fff; cursor: pointer; box-shadow: 0 1px 4px rgba(0,0,0,.25); white-space: nowrap; }
  .pill:hover { background: #9b55d6; }
  .pill:focus-visible { outline: 2px solid #fff; outline-offset: -3px; }
  .pill svg { width: 14px; height: 14px; flex: none; }
  .menu { position: absolute; right: 0; top: 28px; min-width: 220px; max-width: 320px; padding: 4px; border-radius: 8px;
    background: #23202a; box-shadow: 0 6px 20px rgba(0,0,0,.35); display: none; }
  .menu.on { display: block; }
  .menu button { display: block; width: 100%; text-align: left; padding: 7px 9px; border: 0; border-radius: 6px; background: none; color: #eee; cursor: pointer; }
  .menu button:hover, .menu button:focus-visible { background: #3a3445; outline: none; }
  .menu b { display: block; margin-bottom: 3px; }
  .menu span { font-weight: 400; color: #b9b4c2; overflow: hidden; text-overflow: ellipsis; display: block; }
  .note { position: absolute; right: 0; top: 28px; padding: 7px 9px; border-radius: 6px; background: #23202a; color: #eee;
    font-weight: 400; white-space: nowrap; display: none; box-shadow: 0 6px 20px rgba(0,0,0,.35); }
  .note.on { display: block; }
</style>
<button class="pill" title="Carl Vault ile doldur">
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M8 1.5 2.5 3.6v4.1c0 3.2 2.3 5.8 5.5 6.8 3.2-1 5.5-3.6 5.5-6.8V3.6z"/><rect x="5.6" y="7.2" width="4.8" height="3.6" rx=".6" fill="currentColor" stroke="none"/><path d="M6.6 7.2V6a1.4 1.4 0 0 1 2.8 0v1.2"/></svg>
  <span class="label">Carl Vault</span>
</button>
<div class="menu"></div>
<div class="note"></div>`;
  const pill = root.querySelector(".pill");
  const label = root.querySelector(".label");
  const menu = root.querySelector(".menu");
  const noteEl = root.querySelector(".note");

  let state = null; // son find yanıtı
  let anchor = null;
  let busy = false;
  let checkedFor = ""; // hangi adres için soruldu
  let checkedAt = 0;

  function place() {
    if (!anchor || !anchor.isConnected || !visible(anchor)) {
      host.style.display = "none";
      return;
    }
    const r = anchor.getBoundingClientRect();
    const h = 24;
    const w = pill.offsetWidth || 96;
    // Kutunun içinde sağda; kutu kısaysa kutunun hemen üstüne.
    const inside = r.width > w + 120 && r.height >= h + 2;
    const x = inside ? r.right - w - 6 : r.right - w;
    const y = inside ? r.top + (r.height - h) / 2 : r.top - h - 4;
    host.style.left = `${Math.round(x + scrollX)}px`;
    host.style.top = `${Math.round(y + scrollY)}px`;
    host.style.display = "block";
  }

  function note(text, ms) {
    menu.classList.remove("on");
    noteEl.textContent = text;
    noteEl.classList.toggle("on", !!text);
    clearTimeout(note.t);
    if (text && ms) note.t = setTimeout(() => noteEl.classList.remove("on"), ms);
  }

  function hide() {
    host.style.display = "none";
    anchor = null;
  }

  // Bu sayfa için düğme gösterilsin mi?
  const wanted = (r) => r && !r.off && (r.locked || r.closed || (r.ok && (r.logins || []).length > 0));

  async function check() {
    if (busy) return;
    const f = fields();
    if (!f.anchor) {
      if (!pill.matches(":focus")) hide();
      return;
    }
    const key = location.href.split("#")[0];
    if (key !== checkedFor || Date.now() - checkedAt > 20000) {
      checkedFor = key;
      checkedAt = Date.now();
      state = await ask({ op: "vault-find" });
    }
    if (!wanted(state)) {
      hide();
      return;
    }
    anchor = f.anchor;
    if (!host.isConnected) document.documentElement.append(host);
    place();
  }

  async function fillWith(login) {
    note("");
    const r = await ask({ op: "vault-fill", id: login.id });
    if (!r || !r.ok || !r.fill) {
      note((r && r.message) || "CarlUI'ya ulaşılamadı.", 4000);
      return;
    }
    let [user, pass] = r.fill;
    r.fill = null;
    const f = fields();
    if (f.user && user) setValue(f.user, user);
    if (f.pw && pass) setValue(f.pw, pass);
    user = pass = "";
    if (!f.user && !f.pw) note("Giriş kutusu bulunamadı.", 4000);
  }

  function choose(logins) {
    if (logins.length === 1) return fillWith(logins[0]);
    note("");
    menu.replaceChildren(
      ...logins.map((l) => {
        const b = document.createElement("button");
        const n = document.createElement("b");
        n.textContent = l.name;
        const u = document.createElement("span");
        u.textContent = l.username || "kullanıcı adı yok";
        b.append(n, u);
        b.addEventListener("click", (e) => {
          if (!e.isTrusted) return;
          menu.classList.remove("on");
          fillWith(l);
        });
        return b;
      })
    );
    menu.classList.add("on");
    menu.querySelector("button")?.focus();
  }

  // Kilit açılsın diye kasa öne gelir; açılınca (en çok 2 dk) doldur.
  async function unlockThenFill() {
    busy = true;
    label.textContent = "Carl Vault'ta kilidi aç…";
    place();
    try {
      const o = await ask({ op: "vault-open" });
      if (!o || o.off || (!o.ok && !o.closed)) {
        note((o && o.message) || "Carl Vault açılamadı.", 4000);
        return;
      }
      const end = Date.now() + 120000;
      while (Date.now() < end) {
        await new Promise((ok) => setTimeout(ok, 1000));
        const r = await ask({ op: "vault-find" });
        if (!r || r.locked || r.closed || !r.ok) continue; // ya da kasa hâlâ önde
        state = r;
        checkedAt = Date.now();
        const logins = r.logins || [];
        if (logins.length === 0) {
          note("Bu site için Carl Vault'ta kayıt yok.", 4000);
          setTimeout(check, 4200);
          return;
        }
        await choose(logins);
        return;
      }
      note("Süre doldu; tekrar bas.", 4000);
    } finally {
      busy = false;
      label.textContent = "Carl Vault";
      place();
    }
  }

  pill.addEventListener("click", async (e) => {
    // Sayfanın yapay tıklaması sayılmaz.
    if (!e.isTrusted || busy) return;
    e.preventDefault();
    e.stopPropagation();
    if (menu.classList.contains("on")) {
      menu.classList.remove("on");
      return;
    }
    const r = await ask({ op: "vault-find" });
    if (r) state = r;
    if (r && r.ok && (r.logins || []).length) return choose(r.logins);
    if (r && r.ok) return note("Bu site için Carl Vault'ta kayıt yok.", 4000);
    if (r && (r.locked || r.closed)) return unlockThenFill();
    note((r && r.message) || "CarlUI çalışmıyor.", 4000);
  });
  // Tıklama sayfaya ulaşmasın (bazı formlar dış tıklamada kapanır).
  for (const t of ["mousedown", "pointerdown", "mouseup"]) host.addEventListener(t, (e) => e.stopPropagation());
  root.addEventListener("keydown", (e) => {
    if (e.key === "Escape") note("");
  });
  document.addEventListener("mousedown", (e) => {
    if (e.target !== host) menu.classList.remove("on");
  }, true);

  // --- Gönderilen formu yakala ve "kaydedilsin mi?" sor ---------------------
  // Parola kutusu dolu bir form gönderilince (submit, gönder düğmesi ya da
  // Enter) kullanıcı adı ve parola arka plana gider; sayfada saklanmaz. Kasa
  // yeni ya da değişmiş derse sağ üstte küçük bir şerit sorar. Kasaya ancak
  // "Kaydet"e (gerçek tıklama) basılınca yazılır.
  function grab(scope) {
    const pws = [...scope.querySelectorAll("input[type=password]")].filter((e) => e.value);
    if (!pws.length) return null;
    // Kayıt formunda "new-password"; parola değiştirmede [eski, yeni, tekrar].
    const fresh = pws.find((e) => e.autocomplete === "new-password");
    const pw = fresh || (pws.length > 1 && pws[0].value !== pws[1].value ? pws[1] : pws[0]);
    const all = [...scope.querySelectorAll("input")];
    const before = all.slice(0, all.indexOf(pws[0])).filter((e) => textLike(e) && e.value.trim());
    const user =
      before.filter((e) => e.autocomplete === "username" || e.autocomplete === "email" || e.type === "email").pop() ||
      before.filter(userLike).pop() ||
      before.pop() ||
      null;
    const signup = !!fresh || pws.length >= 2 || /sign.?up|regist|kay[ıi]t|uye|üye|join|create|new.?account/i.test(location.pathname + " " + (scope.action || "") + " " + (scope.id || ""));
    return { user: user ? user.value.trim() : "", password: pw.value, signup };
  }

  let sent = ""; // aynı bilgiyi iki kez yollama
  async function captured(target) {
    const scope = target?.closest?.("form") || target?.form || document;
    const g = grab(scope);
    if (!g) return;
    const key = g.user + "\u0000" + g.password;
    if (key === sent) return;
    sent = key;
    const r = await ask({ op: "vault-capture", ...g });
    g.password = "";
    if (r && r.show) offer(r);
  }

  document.addEventListener("submit", (e) => e.isTrusted && captured(e.target), true);
  document.addEventListener("click", (e) => {
    if (!e.isTrusted || e.composedPath().includes(host) || e.composedPath().includes(bar)) return;
    const b = e.target.closest?.("button, input[type=submit], input[type=button], [role=button]");
    if (b && (b.type === "submit" || b.form || b.closest("form") || /giri|log.?in|sign|kay[ıi]t|devam|next|continue|create|oluştur/i.test(b.textContent + " " + (b.value || "")))) captured(b);
  }, true);
  document.addEventListener("keydown", (e) => {
    if (e.isTrusted && e.key === "Enter" && e.target instanceof HTMLInputElement) captured(e.target);
  }, true);

  const bar = document.createElement("carl-vault-save");
  bar.style.cssText = "all:initial;position:fixed;z-index:2147483647;top:12px;right:12px;display:none";
  const barRoot = bar.attachShadow({ mode: "closed" });
  barRoot.innerHTML = `
<style>
  * { box-sizing: border-box; font: 400 13px/1.35 "Segoe UI", system-ui, sans-serif; }
  .box { width: 330px; padding: 12px 12px 10px; border-radius: 10px; background: #23202a; color: #eee; box-shadow: 0 8px 28px rgba(0,0,0,.4); }
  .head { display: flex; align-items: center; gap: 7px; font-weight: 600; color: #d9b8f5; margin-bottom: 6px; }
  .head svg { width: 15px; height: 15px; }
  .msg { overflow-wrap: anywhere; }
  .msg b { font-weight: 600; color: #fff; }
  .row { display: flex; justify-content: flex-end; gap: 6px; margin-top: 10px; }
  button { height: 28px; padding: 0 12px; border: 0; border-radius: 6px; cursor: pointer; font-weight: 600; }
  .yes { background: #b06ce6; color: #fff; }
  .yes:hover { background: #9b55d6; }
  .no { background: #3a3445; color: #eee; }
  .no:hover { background: #48405a; }
  button:focus-visible { outline: 2px solid #fff; outline-offset: -3px; }
</style>
<div class="box" role="dialog" aria-label="Carl Vault">
  <div class="head"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M8 1.5 2.5 3.6v4.1c0 3.2 2.3 5.8 5.5 6.8 3.2-1 5.5-3.6 5.5-6.8V3.6z"/></svg>Carl Vault</div>
  <div class="msg"></div>
  <div class="row"><button class="no">Şimdi değil</button><button class="yes">Kaydet</button></div>
</div>`;
  const barMsg = barRoot.querySelector(".msg");
  const barRow = barRoot.querySelector(".row");
  const yes = barRoot.querySelector(".yes");
  const no = barRoot.querySelector(".no");
  let saving = false;

  function say(parts) {
    // Site ve kullanıcı adı sayfadan gelir: yalnızca metin olarak yazılır.
    barMsg.replaceChildren(...parts.map((p) => (typeof p === "string" ? document.createTextNode(p) : Object.assign(document.createElement("b"), { textContent: p.b }))));
  }

  function closeBar(ms) {
    clearTimeout(closeBar.t);
    closeBar.t = setTimeout(() => (bar.style.display = "none"), ms || 0);
  }

  function offer(r) {
    if (saving) return;
    const who = r.user ? [{ b: r.site }, " · ", { b: r.user }] : [{ b: r.site }];
    if (r.status === "changed") {
      say([...who, " için kayıtlı parola farklı. Carl Vault'ta güncellensin mi?"]);
      yes.textContent = "Güncelle";
    } else if (r.status === "locked" || r.status === "closed") {
      say([...who, " hesabı Carl Vault'a kaydedilsin mi? Kasa kilitli; önce kilidi açman istenecek."]);
      yes.textContent = "Aç ve kaydet";
    } else {
      say([...who, " hesabı Carl Vault'a kaydedilsin mi?"]);
      yes.textContent = "Kaydet";
    }
    barRow.style.display = "flex";
    if (!bar.isConnected) document.documentElement.append(bar);
    bar.style.display = "block";
    clearTimeout(closeBar.t);
  }

  function done(text) {
    say([text]);
    barRow.style.display = "none";
    closeBar(3500);
  }

  yes.addEventListener("click", async (e) => {
    if (!e.isTrusted || saving) return;
    saving = true;
    try {
      let r = await ask({ op: "vault-save" });
      if (r && r.retry) {
        // Kilitli ya da kapalı: kasa öne gelir, parola oraya yazılır; açılınca kaydedilir.
        say(["Carl Vault'ta kilidi aç; açılınca kaydedilecek."]);
        barRow.style.display = "none";
        const o = await ask({ op: "vault-open" });
        if (!o || o.off || (!o.ok && !o.closed)) return done((o && o.message) || "Carl Vault açılamadı.");
        const end = Date.now() + 120000;
        while (Date.now() < end && r && r.retry) {
          await new Promise((ok) => setTimeout(ok, 1000));
          r = await ask({ op: "vault-save" });
        }
      }
      if (r && r.ok) done(r.status === "same" ? "Zaten kayıtlı." : r.message || "Kaydedildi.");
      else done((r && !r.retry && r.message) || "Kaydedilemedi; süre doldu.");
    } finally {
      saving = false;
    }
  });
  no.addEventListener("click", (e) => {
    if (!e.isTrusted) return;
    ask({ op: "vault-dismiss" });
    closeBar();
  });
  for (const t of ["mousedown", "pointerdown", "mouseup", "keydown"]) bar.addEventListener(t, (e) => e.stopPropagation());

  // Form gönderilince sayfa değiştiyse soru yeni sayfada sürer.
  ask({ op: "vault-pending" }).then((r) => r && r.show && offer(r));

  // Sayfa değiştikçe (tek sayfalık uygulamalar) yeniden bak; az sık.
  let pending = 0;
  const soon = () => {
    if (!pending) pending = setTimeout(() => ((pending = 0), check()), 500);
  };
  new MutationObserver((list) => list.some((m) => m.target !== host) && soon()).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["type", "style", "class", "hidden"] });
  addEventListener("scroll", () => anchor && place(), { passive: true, capture: true });
  addEventListener("resize", () => anchor && place(), { passive: true });
  addEventListener("focus", () => ((checkedAt = 0), soon()));
  check();
})();
