// Carl Vault passkey'leri: sayfanın kendi dünyasında (world: "MAIN")
// navigator.credentials.create/get çağrılarını yakalar. "publicKey" istekleri
// aktarıcıya (passkey-relay.js) → arka plana → CarlUI'ye gider; kasa onay
// ister. Kasa kapalıysa, kullanıcı "Tarayıcıya bırak" derse, site ES256 dışında
// algoritma isterse ya da kasada bu siteye passkey yoksa çağrı tarayıcının
// kendi akışına aynen bırakılır. Otomatik doldurma (mediation: "conditional")
// ve güvenlik anahtarı (cross-platform) istekleri hiç yakalanmaz.
//
// Köken ve rpId burada değil, CarlUI'de tarayıcının bildirdiği sekme adresine
// göre denetlenir; bu betiğin gönderdiği hiçbir şeye güvenilmez.

(() => {
  const C = navigator.credentials;
  if (window.top !== window || !C || !window.PublicKeyCredential || window.__carlPasskey) return;
  window.__carlPasskey = true;

  const TAG = "__carlui_pk";
  const nativeCreate = C.create.bind(C);
  const nativeGet = C.get.bind(C);
  const waiting = new Map();
  let seq = 0;

  window.addEventListener("message", (e) => {
    const d = e.data;
    if (e.source !== window || !d || typeof d !== "object" || (d[TAG] !== "res" && d[TAG] !== "ack")) return;
    const w = waiting.get(d.id);
    if (!w) return;
    if (d[TAG] === "ack") {
      w.acked = true;
      return;
    }
    waiting.delete(d.id);
    w.done(d.result);
  });

  // Aktarıcı 1,5 sn içinde almazsa (eklenti güncellendi/kapandı) tarayıcıya bırak.
  const relay = (op, data) =>
    new Promise((done) => {
      const id = `${++seq}:${Math.random()}`;
      const w = { done, acked: false };
      waiting.set(id, w);
      window.postMessage({ [TAG]: "req", id, op, data }, location.origin);
      setTimeout(() => !w.acked && waiting.delete(id) && done(null), 1500);
      setTimeout(() => waiting.delete(id) && done(null), 135000);
    });

  const ABORT = {};
  const race = (p, signal) =>
    !signal ? p : new Promise((ok) => {
      signal.addEventListener("abort", () => ok(ABORT), { once: true });
      p.then(ok);
    });

  const view = (b) => (b instanceof ArrayBuffer ? new Uint8Array(b) : ArrayBuffer.isView(b) ? new Uint8Array(b.buffer, b.byteOffset, b.byteLength) : null);
  const b64 = (b) => {
    const u = view(b);
    if (!u) throw new TypeError("BufferSource");
    let s = "";
    for (let i = 0; i < u.length; i++) s += String.fromCharCode(u[i]);
    return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  };
  const buf = (s) => {
    s = String(s || "").replace(/-/g, "+").replace(/_/g, "/");
    while (s.length % 4) s += "=";
    const r = atob(s);
    const u = new Uint8Array(r.length);
    for (let i = 0; i < r.length; i++) u[i] = r.charCodeAt(i);
    return u.buffer;
  };

  const TRANSPORTS = ["internal", "hybrid"];

  function created(p, wantProps) {
    const ext = wantProps ? { credProps: { rk: true } } : {};
    const response = {
      clientDataJSON: buf(p.client_data),
      attestationObject: buf(p.attestation_object),
      getAuthenticatorData: () => buf(p.authenticator_data),
      getPublicKey: () => buf(p.public_key),
      getPublicKeyAlgorithm: () => -7,
      getTransports: () => TRANSPORTS.slice(),
    };
    Object.setPrototypeOf(response, AuthenticatorAttestationResponse.prototype);
    const cred = {
      id: p.id,
      rawId: buf(p.id),
      type: "public-key",
      authenticatorAttachment: "platform",
      response,
      getClientExtensionResults: () => ext,
      toJSON: () => ({
        id: p.id,
        rawId: p.id,
        type: "public-key",
        authenticatorAttachment: "platform",
        clientExtensionResults: ext,
        response: {
          clientDataJSON: p.client_data,
          attestationObject: p.attestation_object,
          authenticatorData: p.authenticator_data,
          publicKey: p.public_key,
          publicKeyAlgorithm: -7,
          transports: TRANSPORTS.slice(),
        },
      }),
    };
    Object.setPrototypeOf(cred, PublicKeyCredential.prototype);
    return cred;
  }

  function asserted(p) {
    const response = {
      clientDataJSON: buf(p.client_data),
      authenticatorData: buf(p.authenticator_data),
      signature: buf(p.signature),
      userHandle: p.user_handle ? buf(p.user_handle) : null,
    };
    Object.setPrototypeOf(response, AuthenticatorAssertionResponse.prototype);
    const cred = {
      id: p.id,
      rawId: buf(p.id),
      type: "public-key",
      authenticatorAttachment: "platform",
      response,
      getClientExtensionResults: () => ({}),
      toJSON: () => ({
        id: p.id,
        rawId: p.id,
        type: "public-key",
        authenticatorAttachment: "platform",
        clientExtensionResults: {},
        response: { clientDataJSON: p.client_data, authenticatorData: p.authenticator_data, signature: p.signature, userHandle: p.user_handle || null },
      }),
    };
    Object.setPrototypeOf(cred, PublicKeyCredential.prototype);
    return cred;
  }

  async function handle(op, data, options, native, build) {
    const r = await race(relay(op, data), options.signal);
    if (r === ABORT) throw new DOMException("The operation was aborted.", "AbortError");
    if (r && r.ok && r.passkey) return build(r.passkey);
    if (r && r.error) throw new DOMException(r.message || "", r.error);
    return native(options);
  }

  const ids = (list) => (Array.isArray(list) ? list.filter((c) => c && c.type === "public-key").slice(0, 32).map((c) => b64(c.id)) : []);

  C.create = function create(options) {
    const pk = options && options.publicKey;
    if (!pk || options.signal?.aborted || pk.authenticatorSelection?.authenticatorAttachment === "cross-platform") return nativeCreate(options);
    let data;
    try {
      data = {
        rp_id: pk.rp?.id || location.hostname,
        rp_name: pk.rp?.name || "",
        user_name: pk.user?.name || "",
        user_display: pk.user?.displayName || "",
        user_id: b64(pk.user.id),
        challenge: b64(pk.challenge),
        algs: (pk.pubKeyCredParams || []).map((p) => p && p.alg),
        exclude: ids(pk.excludeCredentials),
      };
    } catch {
      return nativeCreate(options);
    }
    const props = !!pk.extensions?.credProps;
    return handle("create", data, options, nativeCreate, (p) => created(p, props));
  };

  C.get = function get(options) {
    const pk = options && options.publicKey;
    if (!pk || options.mediation === "conditional" || options.signal?.aborted) return nativeGet(options);
    let data;
    try {
      data = { rp_id: pk.rpId || location.hostname, challenge: b64(pk.challenge), allow: ids(pk.allowCredentials) };
    } catch {
      return nativeGet(options);
    }
    return handle("get", data, options, nativeGet, asserted);
  };
})();
