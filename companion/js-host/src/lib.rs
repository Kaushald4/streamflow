//! Runs companion adapters as plain JavaScript inside an embedded QuickJS
//! engine, an adapter's `createAdapter(http)`/`findMovie`/`findEpisode`/
//! `extract` all run as real, unmodified JS, the same code the web build's
//! `extractor` package runs under Node.
//!
//! The one thing that *does* need a native implementation is
//! `primitives/wasm-decrypt.ts`, the shared primitive some adapters (Vidsrc2
//! today) use to decrypt a payload by executing a WASM module the target
//! site serves at runtime. Rather than aliasing QuickJS's ArrayBuffer with
//! wasmtime's linear memory (real lifetime risk), this crate resolves an
//! import of that primitive to a native shim backed by
//! `wasm_host::decrypt_with_site_wasm` instead of a real `WebAssembly`
//! polyfill (see `rewrite_wasm_decrypt_import` below). Any future source
//! using the same shared-primitive convention gets this for free.
//!
//! IMPORTANT, this depends on a packaging requirement: the adapter bundle
//! handed to `load_package` must import that primitive from an *external*
//! (unbundled) module, not have it inlined by the bundler. `extractor`'s own
//! `scripts/pack-adapter.mjs` marks that import external so js-host can
//! resolve it to the native shim; adapters that never touch this primitive
//! are unaffected.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use extractor_core::{AdapterManifest, ExtractionFailure, ExtractionInput, ExtractionResult, SourcePage, Stream};
use rquickjs::{
    function::Opt,
    loader::{BuiltinLoader, BuiltinResolver},
    Ctx, Exception, Function, Module, Object, Promise, Runtime, TypedArray, Value,
};
use serde::Deserialize;
use serde_json::json;

/// An installed adapter package: its manifest plus the raw JS bundle text.
/// The bundle is re-evaluated in a fresh QuickJS context on every call,
/// adapters are cheap to parse/run and this sidesteps QuickJS's non-`Send`
/// types ever needing to survive across an `.await` point.
#[derive(Debug, Clone)]
pub struct AdapterPackage {
    pub manifest: AdapterManifest,
    js_source: String,
}

/// Deliberately holds no state, in particular, no `reqwest::blocking::Client`.
/// That client owns its own background Tokio runtime, and dropping one from
/// inside an *async* context (e.g. as part of an `Arc<AppState>` living for a
/// whole server's lifetime, torn down on an async worker thread) panics with
/// "Cannot drop a runtime in a context where blocking is not allowed". Every
/// method here builds its own client locally instead, scoped to a single
/// call, safe as long as callers only ever invoke `extract`/`load_package`
/// via `tokio::task::spawn_blocking`, which is a context blocking is allowed
/// in for both construction and drop.
#[derive(Default)]
pub struct JsHost;

impl JsHost {
    pub fn new() -> Self {
        Self
    }

    /// Parses the manifest and smoke-checks the bundle: it must evaluate as
    /// an ES module and export a callable `createAdapter` before install.
    pub fn load_package(&self, manifest_json: &[u8], js_bytes: &[u8]) -> Result<AdapterPackage> {
        let manifest: AdapterManifest = serde_json::from_slice(manifest_json).context("invalid manifest.json")?;
        let js_source = String::from_utf8(js_bytes.to_vec()).context("adapter entry is not valid UTF-8 JS")?;
        reject_web_only_bundle(&js_source)?;
        validate_bundle(&js_source).context("adapter entry module is invalid")?;
        Ok(AdapterPackage { manifest, js_source })
    }

    /// Runs the full discover → extract pipeline for `input` against
    /// `package`, blocking on real (synchronous) HTTP calls. Call this via
    /// `tokio::task::spawn_blocking` from an async handler, nothing here is
    /// `Send`, but it's all constructed and torn down inside this one call.
    pub fn extract(&self, package: &AdapterPackage, input: &ExtractionInput) -> Result<ExtractionResult> {
        let start = Instant::now();
        let input_json = serde_json::to_string(input)?;
        // SSRF guard: adapter code gets a real, unrestricted-looking HTTP
        // bridge (it needs to reach arbitrary CDN/site hosts), but must
        // never be able to use that to reach the user's LAN.
        let http_client = reqwest::blocking::Client::builder()
            .dns_resolver(std::sync::Arc::new(extractor_core::ssrf::SsrfSafeResolver))
            .build()
            .context("failed to build reqwest client")?;
        let outcome = run_pipeline(&http_client, &package.js_source, &input_json)?;
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        if outcome.ok {
            Ok(ExtractionResult {
                input: input.clone(),
                streams: outcome.streams,
                sources: outcome.source.into_iter().collect(),
                errors: Vec::new(),
                duration_ms,
            })
        } else {
            Ok(ExtractionResult {
                input: input.clone(),
                streams: Vec::new(),
                sources: Vec::new(),
                errors: outcome
                    .errors
                    .into_iter()
                    .map(|e| ExtractionFailure {
                        extractor_id: package.manifest.id.clone(),
                        code: e.code,
                        message: e.message,
                        layer: Some("extraction".to_string()),
                    })
                    .collect(),
                duration_ms,
            })
        }
    }
}

#[derive(Debug, Deserialize)]
struct PipelineOutcome {
    ok: bool,
    #[serde(default)]
    streams: Vec<Stream>,
    #[serde(default)]
    source: Option<SourcePage>,
    #[serde(default)]
    errors: Vec<PipelineError>,
}

#[derive(Debug, Deserialize)]
struct PipelineError {
    #[serde(default = "default_error_code")]
    code: String,
    #[serde(default)]
    message: String,
}

fn default_error_code() -> String {
    "UNKNOWN".to_string()
}

/// Byte blobs handed between host and JS (a downloaded WASM module, decoded
/// ciphertext) never need to be JS-visible arrays, the adapter bundle only
/// ever passes them opaquely between `loadWasmModule` → `instantiateDecryptor`
/// → `decryptWithWasm` (verified against the real vidsrc2 adapter source).
/// So they live here, keyed by a handle id threaded through as a plain number.
type ByteArena = Rc<RefCell<HashMap<u32, Vec<u8>>>>;
type VidukiSessionArena = Rc<RefCell<HashMap<u32, wasm_host::viduki::VidukiBridge>>>;

fn store_bytes(arena: &ByteArena, next_id: &Rc<RefCell<u32>>, bytes: Vec<u8>) -> u32 {
    let mut id_ref = next_id.borrow_mut();
    *id_ref += 1;
    let id = *id_ref;
    arena.borrow_mut().insert(id, bytes);
    id
}

fn validate_bundle(js_source: &str) -> Result<()> {
    with_adapter_context(js_source, |ctx, module| {
        let create_adapter = resolve_create_adapter(ctx, module)?;
        let http = Object::new(ctx.clone()).map_err(|e| describe_js_error(ctx, e))?;
        let adapter: Object = create_adapter.call((http,)).map_err(|e| describe_js_error(ctx, e))?;
        let extract: rquickjs::Result<Function> = adapter.get("extract");
        extract.map_err(|e| describe_js_error(ctx, e))?;
        Ok(())
    })
}

fn run_pipeline(http_client: &reqwest::blocking::Client, js_source: &str, input_json: &str) -> Result<PipelineOutcome> {
    let result_json = with_adapter_context(js_source, |ctx, module| {
        let arena: ByteArena = Rc::new(RefCell::new(HashMap::new()));
        let next_id: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let viduki_sessions: VidukiSessionArena = Rc::new(RefCell::new(HashMap::new()));
        install_host_functions(ctx, http_client.clone(), arena.clone(), next_id.clone(), viduki_sessions.clone())
            .map_err(|e| describe_js_error(ctx, e))?;
        ctx.eval::<(), _>(PRELUDE_JS).map_err(|e| describe_js_error(ctx, e))?;

        let create_adapter = resolve_create_adapter(ctx, module)?;
        let http_obj: Object = ctx.eval("__buildHttp()").map_err(|e| describe_js_error(ctx, e))?;
        let adapter: Object = create_adapter.call((http_obj,)).map_err(|e| describe_js_error(ctx, e))?;

        let runner: Function = ctx.eval(PIPELINE_RUNNER_JS).map_err(|e| describe_js_error(ctx, e))?;
        let promise: Promise = runner.call((adapter, input_json)).map_err(|e| describe_js_error(ctx, e))?;
        let json: String = promise.finish().map_err(|e| describe_js_error(ctx, e))?;
        Ok(json)
    })?;
    serde_json::from_str(&result_json).with_context(|| {
        format!("adapter pipeline returned malformed JSON: {result_json}")
    })
}

/// Sets up a fresh runtime + context with the native `wasm-decrypt-native`
/// module registered, declares the adapter bundle (after rewriting its
/// wasm-decrypt import to point at that native module), and hands both the
/// context and the evaluated module to `body`.
fn with_adapter_context<T>(js_source: &str, body: impl for<'js> FnOnce(&Ctx<'js>, &Module<'js, rquickjs::module::Evaluated>) -> Result<T>) -> Result<T> {
    let runtime = Runtime::new().context("failed to create QuickJS runtime")?;
    let resolver = BuiltinResolver::default()
        .with_module(WASM_DECRYPT_MODULE_NAME)
        .with_module(VIDUKI_WASM_MODULE_NAME);
    let loader = BuiltinLoader::default()
        .with_module(WASM_DECRYPT_MODULE_NAME, WASM_DECRYPT_SHIM_JS)
        .with_module(VIDUKI_WASM_MODULE_NAME, VIDUKI_WASM_SHIM_JS);
    runtime.set_loader(resolver, loader);
    let context = rquickjs::Context::full(&runtime).context("failed to create QuickJS context")?;

    context.with(|ctx| {
        install_crypto_host_functions(&ctx).map_err(|e| describe_js_error(&ctx, e))?;
        ctx.eval::<(), _>(CRYPTO_BOOTSTRAP_JS).map_err(|e| describe_js_error(&ctx, e))?;

        let rewritten = rewrite_wasm_decrypt_import(js_source);
        let declared = Module::declare(ctx.clone(), "adapter", rewritten)
            .map_err(|e| describe_js_error(&ctx, e))
            .context("failed to parse adapter bundle")?;
        let (module, eval_promise) = declared
            .eval()
            .map_err(|e| describe_js_error(&ctx, e))
            .context("failed to evaluate adapter bundle")?;
        let _: Value = eval_promise
            .finish()
            .map_err(|e| describe_js_error(&ctx, e))
            .context("adapter module's top-level evaluation failed")?;
        body(&ctx, &module)
    })
}

fn resolve_create_adapter<'js>(ctx: &Ctx<'js>, module: &Module<'js, rquickjs::module::Evaluated>) -> Result<Function<'js>> {
    module
        .get::<_, Function>("createAdapter")
        .map_err(|e| describe_js_error(ctx, e))
        .context("adapter entry must export createAdapter(http)")
}

/// `rquickjs::Error`'s own `Display` is a generic placeholder for the
/// `Exception` variant ("Exception generated by QuickJS"), the actual
/// thrown value (a real `Error`'s `.message`/`.stack`, or whatever else JS
/// code threw) only lives in the context's exception slot, retrievable via
/// `ctx.catch()`. Without this, every JS-side failure looked identical and
/// gave no way to tell "adapter code has a bug" from "network unreachable"
/// from "missing polyfill."
/// `anyhow::Error`'s plain `Display` only ever prints the top-level
/// `.context(...)` message, silently dropping everything underneath,
/// exactly the kind of generic-wrapper-hides-the-real-error problem
/// `describe_js_error` exists to fix on the QuickJS side. Host functions
/// that convert an `anyhow::Error` into a JS exception message must walk
/// the full chain instead, or a real cause (DNS failure, connection
/// refused, TLS error, timeout, all real, distinct failure modes) shows up
/// to the user as an undifferentiated "HTTP request failed".
fn anyhow_chain(err: &anyhow::Error) -> String {
    err.chain().map(|c| c.to_string()).collect::<Vec<_>>().join(": ")
}

fn describe_js_error(ctx: &Ctx<'_>, err: rquickjs::Error) -> anyhow::Error {
    if !matches!(err, rquickjs::Error::Exception) {
        return anyhow::anyhow!("{err}");
    }

    let caught: Value = ctx.catch();
    if let Some(obj) = caught.as_object() {
        let name: String = obj.get("name").unwrap_or_else(|_| "Error".to_string());
        let message: String = obj.get("message").unwrap_or_default();
        let stack: Option<String> = obj.get("stack").ok();
        return match stack {
            Some(stack) if !stack.is_empty() => anyhow::anyhow!("{name}: {message}\n{stack}"),
            _ => anyhow::anyhow!("{name}: {message}"),
        };
    }
    if let Some(s) = caught.as_string() {
        if let Ok(s) = s.to_string() {
            return anyhow::anyhow!("{s}");
        }
    }
    anyhow::anyhow!("adapter threw a non-Error value: {caught:?}")
}

/// Catches bundles that inlined `primitives/wasm-decrypt.ts` instead of
/// leaving it external. Those bundles call `WebAssembly.compile`/`instantiate`
/// directly, QuickJS has no `WebAssembly` global, so they fail at runtime.
/// `pack-adapter.mjs` marks that import external so js-host can shim it.
fn reject_web_only_bundle(js_source: &str) -> Result<()> {
    if js_source.contains("WebAssembly") {
        anyhow::bail!(
            "This package inlined wasm-decrypt and calls the WebAssembly API \
             directly, which the companion's JS engine (QuickJS) does not have. \
             Rebuild with `pnpm pack-adapters` from the extractor package."
        );
    }
    Ok(())
}

/// Rewrites external WASM primitive imports to native companion shims.
fn rewrite_native_wasm_imports(js_source: &str) -> String {
    let decrypt_re = regex::Regex::new(r#"["'][^"']*wasm-decrypt[^"']*["']"#).expect("valid regex");
    let with_decrypt = decrypt_re
        .replace_all(js_source, format!("\"{WASM_DECRYPT_MODULE_NAME}\""))
        .into_owned();
    let viduki_re = regex::Regex::new(r#"["'][^"']*viduki-wasm[^"']*["']"#).expect("valid regex");
    viduki_re
        .replace_all(&with_decrypt, format!("\"{VIDUKI_WASM_MODULE_NAME}\""))
        .into_owned()
}

fn rewrite_wasm_decrypt_import(js_source: &str) -> String {
    rewrite_native_wasm_imports(js_source)
}

const WASM_DECRYPT_MODULE_NAME: &str = "wasm-decrypt-native";
const VIDUKI_WASM_MODULE_NAME: &str = "viduki-wasm-native";

const WASM_DECRYPT_SHIM_JS: &str = r#"
export async function loadWasmModule(http, wasmUrl) {
  const res = await http.get(wasmUrl, { responseType: "arrayBuffer", timeoutMs: 30000 });
  if (!res.ok) {
    throw new Error("Failed to download WASM (" + res.status + ")");
  }
  return res.data;
}
export async function instantiateDecryptor(module) {
  if (!module || typeof module.__bytesId !== "number") {
    throw new Error("WASM missing exports (native shim received an invalid module handle)");
  }
  return module;
}
export function decryptWithWasm(exports, ciphertext, headerOffset) {
  const offset = headerOffset === undefined ? 12 : headerOffset;
  return __host_wasm_decrypt(exports.__bytesId, ciphertext.__bytesId, offset);
}
export function decodeBase64(data) {
  return { __bytesId: __host_base64_decode_to_handle(data) };
}
"#;

const VIDUKI_WASM_SHIM_JS: &str = r#"
export async function loadVidukiBridge(http) {
  const manifestRes = await http.get("https://www.viduki.net/makima-manifest.json", {
    responseType: "json",
    headers: { referer: "https://www.viduki.net/", origin: "https://www.viduki.net/" },
    timeoutMs: 30000,
  });
  if (!manifestRes.ok || !manifestRes.data || !manifestRes.data.url) {
    throw new Error("Failed to load Viduki WASM manifest");
  }
  const wasmUrl = new URL(manifestRes.data.url, "https://www.viduki.net/makima-manifest.json").href;
  const wasmRes = await http.get(wasmUrl, {
    responseType: "arrayBuffer",
    headers: { referer: "https://www.viduki.net/", origin: "https://www.viduki.net/" },
    timeoutMs: 30000,
  });
  if (!wasmRes.ok || !wasmRes.data || typeof wasmRes.data.__bytesId !== "number") {
    throw new Error("Failed to download Viduki WASM (" + wasmRes.status + ")");
  }
  const handle = __host_viduki_create(wasmRes.data.__bytesId);
  return {
    reset() { __host_viduki_reset(handle); },
    decryptPepper(nonceId, bucket, ivId, ctId, tagId) {
      __host_viduki_decrypt_pepper(handle, nonceId.__bytesId, bucket, ivId.__bytesId, ctId.__bytesId, tagId.__bytesId);
    },
    decryptEnvelope(envelope, clientNonceHex, requestIdHex) {
      return __host_viduki_decrypt_envelope(handle, JSON.stringify(envelope), clientNonceHex, requestIdHex);
    },
    dropPepper() { __host_viduki_drop(handle); },
  };
}
export function applyPepperKey(bridge, sessionNonce, pepper) {
  bridge.reset();
  bridge.decryptPepper(
    { __bytesId: __host_hex_to_handle(sessionNonce) },
    pepper.bucket,
    { __bytesId: __host_hex_to_handle(pepper.iv) },
    { __bytesId: __host_hex_to_handle(pepper.ct) },
    { __bytesId: __host_hex_to_handle(pepper.tag) },
  );
}
"#;

/// `crypto-js` resolves `global.crypto` once at module load. Install this
/// before evaluating any adapter bundle that depends on it.
const CRYPTO_BOOTSTRAP_JS: &str = r#"
if (typeof globalThis.global === "undefined") globalThis.global = globalThis;
const __crypto = globalThis.crypto ?? {};
if (typeof __crypto.getRandomValues !== "function") {
  __crypto.getRandomValues = (typedArray) => {
    const bytes = __host_crypto_random_bytes(typedArray.length);
    for (let i = 0; i < typedArray.length; i++) typedArray[i] = bytes[i];
    return typedArray;
  };
}
globalThis.crypto = __crypto;
globalThis.global.crypto = __crypto;
"#;

/// Minimal, targeted polyfills, only what real adapter code (verified via
/// the actual cineby/vidsrc2 source) touches: `Buffer.from(str,'base64')`,
/// `TextDecoder`/`TextEncoder`, and a `URL`/`URLSearchParams` pair covering absolute-URL
/// parsing, `.searchParams.get/set`, and `.toString()`. QuickJS itself
/// already provides real `Uint8Array`/`JSON`/`Promise`/etc. natively.
const PRELUDE_JS: &str = r#"
const Buffer = {
  from(input, encoding) {
    if (encoding && encoding !== "base64") {
      throw new Error("Buffer.from: unsupported encoding " + encoding);
    }
    return __host_base64_to_bytes(String(input));
  },
};

class TextDecoder {
  constructor(encoding) { this.encoding = encoding || "utf-8"; }
  decode(bytes) { return __host_utf8_decode(bytes); }
}

class TextEncoder {
  encode(input) { return __host_utf8_encode(String(input)); }
}

class URLSearchParams {
  constructor(init) {
    this._pairs = [];
    if (typeof init === "string") {
      const s = init.startsWith("?") ? init.slice(1) : init;
      if (s.length) {
        for (const part of s.split("&")) {
          if (!part) continue;
          const eq = part.indexOf("=");
          const k = eq === -1 ? part : part.slice(0, eq);
          const v = eq === -1 ? "" : part.slice(eq + 1);
          this._pairs.push([
            decodeURIComponent(k.replace(/\+/g, " ")),
            decodeURIComponent(v.replace(/\+/g, " ")),
          ]);
        }
      }
    } else if (init && typeof init === "object") {
      for (const k of Object.keys(init)) this._pairs.push([k, String(init[k])]);
    }
  }
  get(key) { const p = this._pairs.find(([k]) => k === key); return p ? p[1] : null; }
  getAll(key) { return this._pairs.filter(([k]) => k === key).map(([, v]) => v); }
  set(key, value) {
    let found = false;
    this._pairs = this._pairs.filter(([k]) => k !== key || !(found = true) || false).concat(found ? [] : []);
    this._pairs = this._pairs.filter(([k]) => k !== key);
    this._pairs.push([key, String(value)]);
  }
  append(key, value) { this._pairs.push([key, String(value)]); }
  delete(key) { this._pairs = this._pairs.filter(([k]) => k !== key); }
  has(key) { return this._pairs.some(([k]) => k === key); }
  toString() {
    return this._pairs.map(([k, v]) => encodeURIComponent(k) + "=" + encodeURIComponent(v)).join("&");
  }
  [Symbol.iterator]() { return this._pairs[Symbol.iterator](); }
}

class URL {
  constructor(input, base) {
    let s = String(input);
    if (base && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(s)) {
      const b = base instanceof URL ? base.toString() : String(base);
      if (s.startsWith("//")) {
        s = b.slice(0, b.indexOf(":") + 1) + s;
      } else if (s.startsWith("/")) {
        const m = b.match(/^([a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^/]+)/);
        s = (m ? m[1] : b) + s;
      } else {
        s = b.slice(0, b.lastIndexOf("/") + 1) + s;
      }
    }
    const m = s.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/);
    if (!m) throw new TypeError("Invalid URL: " + s);
    this.protocol = m[1] + ":";
    let authority = m[2];
    let username = "", password = "", host = authority;
    if (authority.includes("@")) {
      const at = authority.lastIndexOf("@");
      const userinfo = authority.slice(0, at);
      host = authority.slice(at + 1);
      const colon = userinfo.indexOf(":");
      username = colon === -1 ? userinfo : userinfo.slice(0, colon);
      password = colon === -1 ? "" : userinfo.slice(colon + 1);
    }
    this.username = username;
    this.password = password;
    let hostname = host, port = "";
    const portMatch = host.match(/^(.*):(\d+)$/);
    if (portMatch && !host.startsWith("[")) {
      hostname = portMatch[1];
      port = portMatch[2];
    }
    this.hostname = hostname;
    this.port = port;
    this.host = port ? hostname + ":" + port : hostname;
    this.pathname = m[3] || "/";
    this.search = m[4] || "";
    this.hash = m[5] || "";
    this.searchParams = new URLSearchParams(this.search);
  }
  get href() { return this.toString(); }
  set href(v) { const u = new URL(v); Object.assign(this, u); }
  get origin() { return this.protocol + "//" + this.host; }
  toString() {
    const auth = this.username ? this.username + (this.password ? ":" + this.password : "") + "@" : "";
    const qs = this.searchParams.toString();
    const search = qs ? "?" + qs : "";
    return this.protocol + "//" + auth + this.host + this.pathname + search + this.hash;
  }
  toJSON() { return this.toString(); }
}

const console = {
  log: (...a) => __host_console_log("log", a.map(String).join(" ")),
  info: (...a) => __host_console_log("info", a.map(String).join(" ")),
  warn: (...a) => __host_console_log("warn", a.map(String).join(" ")),
  error: (...a) => __host_console_log("error", a.map(String).join(" ")),
};

const __DEFAULT_UA__ = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

function __buildHttp() {
  function doRequest(method, url, options) {
    options = options || {};
    let finalUrl = url;
    if (options.query) {
      const usp = new URLSearchParams();
      for (const k of Object.keys(options.query)) {
        const v = options.query[k];
        if (v === undefined || v === null) continue;
        usp.set(k, String(v));
      }
      const qs = usp.toString();
      if (qs) finalUrl += (finalUrl.includes("?") ? "&" : "?") + qs;
    }
    const headers = Object.assign({ "user-agent": __DEFAULT_UA__ }, options.headers || {});
    const responseType = options.responseType || "text";
    const timeoutMs = options.timeoutMs || 15000;
    const body = options.body != null ? String(options.body) : "";
    const raw = __host_http_request(method, finalUrl, JSON.stringify(headers), body, responseType, timeoutMs);
    return JSON.parse(raw);
  }
  return {
    request: async function (url, options) { return doRequest((options && options.method) || "GET", url, options); },
    get: async function (url, options) { return doRequest("GET", url, options); },
    post: async function (url, body, options) { return doRequest("POST", url, Object.assign({}, options, { body })); },
    head: async function (url, options) { return doRequest("HEAD", url, Object.assign({}, options, { responseType: "raw" })); },
  };
}
"#;

/// Runs the adapter's discover → extract flow and returns a JSON-encoded
/// `PipelineOutcome`. This is *our* glue, not adapter code.
const PIPELINE_RUNNER_JS: &str = r#"
(function (adapter, inputJson) {
  return (async () => {
    const input = JSON.parse(inputJson);
    let sources;
    try {
      if (input.kind === "movie" && adapter.findMovie) {
        sources = await adapter.findMovie(input);
      } else if (input.kind === "episode" && adapter.findEpisode) {
        sources = await adapter.findEpisode(input);
      } else if (input.kind === "short_drama" && adapter.findShortDrama) {
        sources = await adapter.findShortDrama(input);
      } else {
        return JSON.stringify({ ok: false, errors: [{ code: "UNSUPPORTED", message: "Adapter cannot handle input kind: " + input.kind }] });
      }
    } catch (e) {
      return JSON.stringify({ ok: false, errors: [{ code: (e && e.code) || "UNKNOWN", message: String((e && e.message) || e) }] });
    }
    if (!sources || sources.length === 0) {
      return JSON.stringify({ ok: false, errors: [{ code: "NO_SOURCE", message: "Adapter found no source pages" }] });
    }
    const errors = [];
    for (const source of sources) {
      try {
        const streams = await adapter.extract(source);
        return JSON.stringify({ ok: true, streams, source, errors });
      } catch (e) {
        errors.push({ code: (e && e.code) || "UNKNOWN", message: String((e && e.message) || e) });
      }
    }
    return JSON.stringify({ ok: false, errors });
  })();
})
"#;

fn install_crypto_host_functions(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    globals.set(
        "__host_crypto_random_bytes",
        Function::new(ctx.clone(), crypto_random_bytes)?,
    )?;
    Ok(())
}

fn crypto_random_bytes<'js>(ctx: Ctx<'js>, len: u32) -> rquickjs::Result<TypedArray<'js, u8>> {
    use rand::RngCore;
    let n = usize::try_from(len.min(1_048_576)).unwrap_or(0);
    let mut buf = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut buf);
    TypedArray::new_copy(ctx, &buf)
}

fn install_host_functions(
    ctx: &Ctx<'_>,
    http: reqwest::blocking::Client,
    arena: ByteArena,
    next_id: Rc<RefCell<u32>>,
    viduki_sessions: VidukiSessionArena,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    globals.set(
        "__host_console_log",
        Function::new(ctx.clone(), |level: String, msg: String| {
            eprintln!("[adapter:{level}] {msg}");
        })?,
    )?;

    globals.set("__host_base64_to_bytes", Function::new(ctx.clone(), base64_to_bytes)?)?;

    globals.set("__host_utf8_encode", Function::new(ctx.clone(), utf8_encode_bytes)?)?;

    globals.set(
        "__host_altcha_solve",
        Function::new(ctx.clone(), |ctx: Ctx<'_>, salt: String, challenge: String, max_number: u32| -> rquickjs::Result<u32> {
            altcha_solve(&salt, &challenge, max_number).map_err(|e| Exception::throw_type(&ctx, &e))
        })?,
    )?;

    globals.set(
        "__host_primeflix_decrypt_url",
        Function::new(ctx.clone(), |ctx: Ctx<'_>, base64url: String| -> rquickjs::Result<String> {
            primeflix_decrypt_url(&base64url).map_err(|e| Exception::throw_type(&ctx, &e))
        })?,
    )?;

    globals.set(
        "__host_utf8_decode",
        Function::new(ctx.clone(), |bytes: TypedArray<'_, u8>| -> String {
            String::from_utf8_lossy(bytes.as_ref()).into_owned()
        })?,
    )?;

    {
        let arena = arena.clone();
        let next_id = next_id.clone();
        globals.set(
            "__host_base64_decode_to_handle",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, s: String| -> rquickjs::Result<u32> {
                let bytes = decode_base64_loose(&s).map_err(|e| Exception::throw_type(&ctx, &e))?;
                Ok(store_bytes(&arena, &next_id, bytes))
            })?,
        )?;
    }

    {
        let arena = arena.clone();
        let next_id = next_id.clone();
        globals.set(
            "__host_hex_to_handle",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, hex: String| -> rquickjs::Result<u32> {
                let bytes = decode_hex_loose(&hex).map_err(|e| Exception::throw_type(&ctx, &e))?;
                Ok(store_bytes(&arena, &next_id, bytes))
            })?,
        )?;
    }

    {
        let arena = arena.clone();
        let viduki_sessions = viduki_sessions.clone();
        let next_id = next_id.clone();
        globals.set(
            "__host_viduki_create",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, wasm_id: u32| -> rquickjs::Result<u32> {
                let map = arena.borrow();
                let wasm_bytes = map
                    .get(&wasm_id)
                    .ok_or_else(|| Exception::throw_type(&ctx, "unknown wasm bytes handle"))?;
                let bridge = wasm_host::viduki::VidukiBridge::new(wasm_bytes)
                    .map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))?;
                let mut id_ref = next_id.borrow_mut();
                *id_ref += 1;
                let id = *id_ref;
                viduki_sessions.borrow_mut().insert(id, bridge);
                Ok(id)
            })?,
        )?;
    }

    {
        let viduki_sessions = viduki_sessions.clone();
        globals.set(
            "__host_viduki_reset",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, session_id: u32| -> rquickjs::Result<()> {
                let mut sessions = viduki_sessions.borrow_mut();
                let bridge = sessions
                    .get_mut(&session_id)
                    .ok_or_else(|| Exception::throw_type(&ctx, "unknown viduki session"))?;
                bridge.reset().map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))
            })?,
        )?;
    }

    {
        let arena = arena.clone();
        let viduki_sessions = viduki_sessions.clone();
        globals.set(
            "__host_viduki_decrypt_pepper",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>,
                      session_id: u32,
                      nonce_id: u32,
                      bucket: f64,
                      iv_id: u32,
                      ct_id: u32,
                      tag_id: u32|
                      -> rquickjs::Result<()> {
                    let bytes_map = arena.borrow();
                    let nonce = bytes_map
                        .get(&nonce_id)
                        .ok_or_else(|| Exception::throw_type(&ctx, "unknown nonce handle"))?;
                    let iv = bytes_map
                        .get(&iv_id)
                        .ok_or_else(|| Exception::throw_type(&ctx, "unknown iv handle"))?;
                    let ct = bytes_map
                        .get(&ct_id)
                        .ok_or_else(|| Exception::throw_type(&ctx, "unknown ct handle"))?;
                    let tag = bytes_map
                        .get(&tag_id)
                        .ok_or_else(|| Exception::throw_type(&ctx, "unknown tag handle"))?;
                    let mut sessions = viduki_sessions.borrow_mut();
                    let bridge = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| Exception::throw_type(&ctx, "unknown viduki session"))?;
                    bridge
                        .decrypt_pepper(nonce, bucket as u64, iv, ct, tag)
                        .map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))
                },
            )?,
        )?;
    }

    {
        let viduki_sessions = viduki_sessions.clone();
        globals.set(
            "__host_viduki_decrypt_envelope",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>,
                      session_id: u32,
                      envelope_json: String,
                      client_nonce_hex: String,
                      request_id_hex: String|
                      -> rquickjs::Result<String> {
                    #[derive(Deserialize)]
                    struct EnvelopeJson {
                        sn: String,
                        tb: u64,
                        iv1: String,
                        iv2: String,
                        wk: String,
                        tag1: String,
                        tag2: String,
                        ct: String,
                    }
                    let parsed: EnvelopeJson = serde_json::from_str(&envelope_json)
                        .map_err(|e| Exception::throw_type(&ctx, &format!("invalid envelope JSON: {e}")))?;
                    let envelope = wasm_host::viduki::VidukiEnvelope {
                        sn: parsed.sn,
                        tb: parsed.tb,
                        iv1: parsed.iv1,
                        iv2: parsed.iv2,
                        wk: parsed.wk,
                        tag1: parsed.tag1,
                        tag2: parsed.tag2,
                        ct: parsed.ct,
                    };
                    let mut sessions = viduki_sessions.borrow_mut();
                    let bridge = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| Exception::throw_type(&ctx, "unknown viduki session"))?;
                    bridge
                        .decrypt_envelope(&envelope, &client_nonce_hex, &request_id_hex)
                        .map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))
                },
            )?,
        )?;
    }

    {
        let viduki_sessions = viduki_sessions.clone();
        globals.set(
            "__host_viduki_drop",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, session_id: u32| -> rquickjs::Result<()> {
                let mut sessions = viduki_sessions.borrow_mut();
                let mut bridge = sessions
                    .remove(&session_id)
                    .ok_or_else(|| Exception::throw_type(&ctx, "unknown viduki session"))?;
                bridge.drop_pepper().map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))
            })?,
        )?;
    }

    {
        let arena = arena.clone();
        globals.set(
            "__host_wasm_decrypt",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, wasm_id: u32, cipher_id: u32, header_offset: u32| -> rquickjs::Result<String> {
                let map = arena.borrow();
                let wasm_bytes = map.get(&wasm_id).ok_or_else(|| Exception::throw_type(&ctx, "unknown wasm module handle"))?;
                let cipher_bytes = map.get(&cipher_id).ok_or_else(|| Exception::throw_type(&ctx, "unknown ciphertext handle"))?;
                wasm_host::decrypt_with_site_wasm(wasm_bytes, cipher_bytes, header_offset)
                    .map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))
            })?,
        )?;
    }

    {
        let arena = arena.clone();
        let next_id = next_id.clone();
        globals.set(
            "__host_http_request",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, method: String, url: String, headers_json: String, body: String, response_type: String, timeout_ms: Opt<u32>| -> rquickjs::Result<String> {
                    do_http_request(&http, &arena, &next_id, &method, &url, &headers_json, &body, &response_type, timeout_ms.0.unwrap_or(15_000))
                        .map_err(|e| Exception::throw_type(&ctx, &anyhow_chain(&e)))
                },
            )?,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn do_http_request(
    client: &reqwest::blocking::Client,
    arena: &ByteArena,
    next_id: &Rc<RefCell<u32>>,
    method: &str,
    url: &str,
    headers_json: &str,
    body: &str,
    response_type: &str,
    timeout_ms: u32,
) -> Result<String> {
    extractor_core::ssrf::check_target_url(url).map_err(|e| anyhow::anyhow!("{e}"))?;

    let method = reqwest::Method::from_bytes(method.as_bytes()).context("invalid HTTP method")?;
    let headers: HashMap<String, String> = serde_json::from_str(headers_json).unwrap_or_default();
    let mut header_map = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(name), Ok(value)) = (reqwest::header::HeaderName::from_bytes(k.as_bytes()), reqwest::header::HeaderValue::from_str(&v)) {
            header_map.insert(name, value);
        }
    }

    let mut req = client.request(method, url).headers(header_map).timeout(Duration::from_millis(u64::from(timeout_ms)));
    if !body.is_empty() {
        req = req.body(body.to_string());
    }
    let resp = req.send().context("HTTP request failed")?;

    let status = resp.status().as_u16();
    let ok = resp.status().is_success();
    let final_url = resp.url().to_string();
    let bytes = resp.bytes().context("failed to read response body")?.to_vec();

    let (data, text) = match response_type {
        "arrayBuffer" => {
            let id = store_bytes(arena, next_id, bytes);
            (json!({ "__bytesId": id }), None)
        }
        "raw" => (serde_json::Value::Null, None),
        "json" => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let data = serde_json::from_str::<serde_json::Value>(&text).unwrap_or(serde_json::Value::Null);
            (data, Some(text))
        }
        _ => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            (serde_json::Value::String(text.clone()), Some(text))
        }
    };

    let response = json!({
        "ok": ok,
        "status": status,
        "headers": {},
        "url": final_url,
        "data": data,
        "text": text,
    });
    Ok(response.to_string())
}

/// A free function (not a closure) so its `'js` lifetime is properly
/// quantified via HRTB when passed to `Function::new`, a closure with an
/// explicit `Ctx<'_>` parameter and a `'js`-tied return type infers two
/// distinct, non-unifying lifetimes instead.
fn base64_to_bytes<'js>(ctx: Ctx<'js>, s: String) -> rquickjs::Result<TypedArray<'js, u8>> {
    let bytes = decode_base64_loose(&s).map_err(|e| Exception::throw_type(&ctx, &e))?;
    TypedArray::new_copy(ctx, &bytes)
}

fn utf8_encode_bytes<'js>(ctx: Ctx<'js>, s: String) -> rquickjs::Result<TypedArray<'js, u8>> {
    TypedArray::new_copy(ctx, s.as_bytes())
}

/// Native ALTCHA PoW, pure-JS SHA256 in QuickJS is far too slow (appears hung).
fn altcha_solve(salt: &str, challenge: &str, max_number: u32) -> Result<u32, String> {
    use sha2::{Digest, Sha256};

    let target = challenge.trim().to_lowercase();
    let salt_bytes = salt.as_bytes();

    for n in 0..=max_number {
        let num = n.to_string();
        let mut hasher = Sha256::new();
        hasher.update(salt_bytes);
        hasher.update(num.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        if hash == target {
            return Ok(n);
        }
    }
    Err("ALTCHA solution not found".into())
}

/// AES-256-GCM decrypt for Primeflix API `url` fields (matches raw_movies_series/primeflix).
fn primeflix_decrypt_url(base64url: &str) -> Result<String, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use base64::Engine;

    const KEY_HEX: &str =
        "7f3e9c2a8b5d1f4e6a9c3b7d2e5f8a1c4b6d9e2f5a8c1b4d7e9f2a5c8b1d4e7f";

    let mut b64 = base64url.replace('-', "+").replace('_', "/");
    match b64.len() % 4 {
        2 => b64.push_str("=="),
        3 => b64.push('='),
        1 => return Err("invalid base64url length".into()),
        _ => {}
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .map_err(|e| format!("base64 decode: {e}"))?;

    if data.len() < 28 {
        return Err("ciphertext too short".into());
    }

    let iv = &data[..12];
    let tag = &data[data.len() - 16..];
    let ciphertext = &data[12..data.len() - 16];

    let key = decode_hex_loose(KEY_HEX)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("invalid AES key: {e}"))?;
    let nonce = Nonce::from_slice(iv);

    let mut payload = ciphertext.to_vec();
    payload.extend_from_slice(tag);

    let plain = cipher
        .decrypt(nonce, payload.as_ref())
        .map_err(|e| format!("AES-GCM decrypt failed: {e}"))?;

    String::from_utf8(plain).map_err(|e| format!("plaintext not utf-8: {e}"))
}

fn decode_hex_loose(s: &str) -> std::result::Result<Vec<u8>, String> {
    let hex = s.trim();
    if hex.len() % 2 != 0 {
        return Err("invalid hex length".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("invalid hex: {e}")))
        .collect()
}

fn decode_base64_loose(s: &str) -> std::result::Result<Vec<u8>, String> {
    use base64::Engine;
    let s = s.trim();
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(s))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s))
        .map_err(|e| format!("invalid base64: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primeflix_decrypt_roundtrip() {
        use aes_gcm::{
            aead::{Aead, AeadCore, KeyInit, OsRng},
            Aes256Gcm,
        };
        use base64::Engine;

        const KEY_HEX: &str =
            "7f3e9c2a8b5d1f4e6a9c3b7d2e5f8a1c4b6d9e2f5a8c1b4d7e9f2a5c8b1d4e7f";
        let key = decode_hex_loose(KEY_HEX).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let plaintext = b"https://cdn.example.com/stream.m3u8";
        let encrypted = cipher.encrypt(&nonce, plaintext.as_ref()).unwrap();

        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&encrypted);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&blob);
        let b64url = b64.replace('+', "-").replace('/', "_").trim_end_matches('=').to_string();

        let out = primeflix_decrypt_url(&b64url).unwrap();
        assert_eq!(out, "https://cdn.example.com/stream.m3u8");
    }

    const TRIVIAL_ADAPTER_JS: &str = r#"
        export function createAdapter(http) {
            return {
                async findMovie(input) {
                    return [{ extractorId: "test", url: "test://" + input.imdbId, kind: "movie", meta: {} }];
                },
                async extract(source) {
                    return [{
                        url: "https://example.com/stream.m3u8",
                        type: "hls",
                        source: { extractor: "test", version: "1.0.0" },
                    }];
                },
            };
        }
    "#;

    #[test]
    fn loads_and_extracts_a_trivial_adapter() {
        let host = JsHost::new();
        let manifest_json = br#"{
            "id": "test-adapter", "name": "Test", "version": "0.1.0", "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false, "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let package = host.load_package(manifest_json, TRIVIAL_ADAPTER_JS.as_bytes()).unwrap();

        let input = ExtractionInput::Movie {
            imdb_id: Some("tt1234567".to_string()),
            tmdb_id: None,
            title: None,
            year: None,
        };
        let result = host.extract(&package, &input).unwrap();
        assert!(result.errors.is_empty(), "unexpected errors: {:?}", result.errors);
        assert_eq!(result.streams.len(), 1);
        assert_eq!(result.streams[0].url, "https://example.com/stream.m3u8");
    }

    #[test]
    fn crypto_bootstrap_satisfies_crypto_js_random_lookup() {
        // crypto-js captures `global.crypto` at module load, bootstrap must
        // run before the adapter bundle is evaluated.
        const ADAPTER_JS: &str = r#"
            var crypto;
            if (typeof global !== "undefined" && global.crypto) {
                crypto = global.crypto;
            }
            function needRandom() {
                if (crypto && typeof crypto.getRandomValues === "function") {
                    return crypto.getRandomValues(new Uint32Array(1))[0];
                }
                throw new Error("Native crypto module could not be used to get secure random number.");
            }
            export function createAdapter(http) {
                return {
                    async findMovie(input) {
                        return [{ extractorId: "test", url: "test://x", kind: "movie", meta: {} }];
                    },
                    async extract() {
                        needRandom();
                        return [{
                            url: "https://example.com/stream.m3u8",
                            type: "hls",
                            source: { extractor: "test", version: "1.0.0" },
                        }];
                    },
                };
            }
        "#;
        let host = JsHost::new();
        let manifest_json = br#"{
            "id": "crypto-test", "name": "CryptoTest", "version": "0.1.0", "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false, "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let package = host.load_package(manifest_json, ADAPTER_JS.as_bytes()).unwrap();
        let input = ExtractionInput::Movie {
            imdb_id: Some("tt1".into()),
            tmdb_id: None,
            title: None,
            year: None,
        };
        let result = host.extract(&package, &input).unwrap();
        assert!(result.errors.is_empty(), "crypto bootstrap failed: {:?}", result.errors);
        assert_eq!(result.streams.len(), 1);
    }

    #[test]
    fn http_bridge_refuses_to_reach_loopback() {
        // A malicious or CSRF-triggered adapter must not be able to use its
        // http bridge to probe the user's own machine/LAN.
        const SSRF_ADAPTER_JS: &str = r#"
            export function createAdapter(http) {
                return {
                    async findMovie(input) { return [{ extractorId: "test", url: "test://x", kind: "movie", meta: {} }]; },
                    async extract(source) {
                        return [{
                            url: (await http.get("http://127.0.0.1:1/definitely-not-reachable")).text,
                            type: "hls",
                            source: { extractor: "test", version: "1.0.0" },
                        }];
                    },
                };
            }
        "#;
        let host = JsHost::new();
        let manifest_json = br#"{
            "id": "ssrf-test-adapter", "name": "SsrfTest", "version": "0.1.0", "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false, "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let package = host.load_package(manifest_json, SSRF_ADAPTER_JS.as_bytes()).unwrap();
        let input = ExtractionInput::Movie { imdb_id: Some("tt1".into()), tmdb_id: None, title: None, year: None };
        let result = host.extract(&package, &input).unwrap();
        assert!(result.streams.is_empty(), "SSRF-blocked request must not produce a stream");
        assert!(!result.errors.is_empty(), "expected the blocked fetch to surface as an error");
    }

    #[test]
    fn rejects_a_bundle_missing_create_adapter() {
        let host = JsHost::new();
        let manifest_json = br#"{
            "id": "broken", "name": "Broken", "version": "0.1.0", "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false, "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let result = host.load_package(manifest_json, b"export function notCreateAdapter() {}");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_web_target_bundle_that_calls_webassembly_directly() {
        let host = JsHost::new();
        let manifest_json = br#"{
            "id": "web-only", "name": "WebOnly", "version": "0.1.0", "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false, "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let js = b"export function createAdapter(http) { return { extract: async () => { await WebAssembly.compile(new Uint8Array()); return []; } }; }";
        let err = host.load_package(manifest_json, js).unwrap_err();
        assert!(format!("{err:#}").contains("pack-adapters"), "error should point at the fix: {err:#}");
    }

    #[test]
    fn wasm_decrypt_native_module_round_trips_through_the_shared_primitive() {
        // Real end-to-end path: an adapter bundle imports the shared
        // primitive by its normal relative path, js-host rewrites that
        // import to the native shim, and `decryptWithWasm` runs the module
        // through wasm_host's wasmtime pipeline. The module here uses the
        // exact alloc/decrypt/memory ABI `decrypt_with_site_wasm` expects,
        // as an identity transform (decrypt returns its input unchanged) so
        // the test only needs to assert the plumbing works, not any
        // particular site's crypto.
        //
        // This deliberately skips `loadWasmModule`'s real HTTP fetch (no
        // live site to fetch a WASM module from in a unit test) and instead
        // feeds the module bytes in directly via the same arena-handle path
        // `loadWasmModule` would populate, the SSRF guard on the adapter's
        // `http` bridge correctly refuses loopback/private targets, which a
        // test HTTP server would be, so exercising the fetch itself isn't
        // possible here without weakening that guard.
        const IDENTITY_DECRYPT_WAT: &str = r#"
            (module
              (memory (export "memory") 1)
              (func (export "alloc") (param $len i32) (result i32) (i32.const 0))
              (func (export "decrypt") (param $ptr i32) (param $len i32) (result i32) (local.get $len)))
        "#;
        let wasm_b64 = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(IDENTITY_DECRYPT_WAT.as_bytes())
        };

        let adapter_js = format!(
            r#"
            import {{ instantiateDecryptor, decryptWithWasm, decodeBase64 }} from "../../primitives/wasm-decrypt.js";
            export function createAdapter(http) {{
                return {{
                    async findMovie(input) {{
                        return [{{ extractorId: "test", url: "test://x", kind: "movie", meta: {{}} }}];
                    }},
                    async extract(source) {{
                        const moduleBytes = decodeBase64("{wasm_b64}");
                        const exports = await instantiateDecryptor(moduleBytes);
                        const ciphertext = decodeBase64("aGVsbG8=");
                        const plaintext = decryptWithWasm(exports, ciphertext, 0);
                        return [{{ url: "https://example.com/" + plaintext, type: "hls", source: {{ extractor: "test", version: "1.0.0" }} }}];
                    }},
                }};
            }}
            "#
        );

        let host = JsHost::new();
        let manifest_json = br#"{
            "id": "wasm-test-adapter", "name": "WasmTest", "version": "0.1.0", "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false, "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let package = host.load_package(manifest_json, adapter_js.as_bytes()).unwrap();
        let input = ExtractionInput::Movie { imdb_id: Some("tt1".into()), tmdb_id: None, title: None, year: None };
        let result = host.extract(&package, &input).unwrap();
        assert!(result.errors.is_empty(), "unexpected errors: {:?}", result.errors);
        assert_eq!(result.streams.len(), 1);
        // "aGVsbG8=" is base64 for "hello"; the identity decryptor echoes it back.
        assert_eq!(result.streams[0].url, "https://example.com/hello");
    }
}
