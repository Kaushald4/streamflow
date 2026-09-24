//! Native replacement for `primitives/wasm-decrypt.ts`, the one shared,
//! named primitive some adapters (Vidsrc2 today, potentially others later)
//! use to decrypt a payload by executing a WASM module the *target site*
//! serves at runtime. This is not a general WebAssembly polyfill: rather
//! than aliasing QuickJS's ArrayBuffer with wasmtime's linear memory (real
//! lifetime risk, wasm memory can grow/reallocate mid-call), the whole
//! alloc → write → decrypt → read sequence happens atomically here in Rust,
//! with no JS-visible raw memory at all. `js-host` swaps this in wherever an
//! adapter bundle imports that primitive as an external (unbundled) module
//! (see the packaging requirement documented in `js-host`'s crate docs),
//! this is the *only* thing about an adapter that ever needs a native
//! implementation; everything else in the adapter runs as real, unmodified
//! JS.

pub mod viduki;

use anyhow::{Context, Result};
use wasmtime::{Engine, Instance, Module, Store};

/// Mirrors `decryptWithWasm` in `primitives/wasm-decrypt.ts` exactly: the
/// site's module exports `memory`/`alloc(len) -> ptr`/`decrypt(ptr, len) ->
/// outLen`; plaintext bytes land at `ptr + header_offset` once `decrypt`
/// returns. `header_offset` defaults to 12 on the JS side; callers here pass
/// it explicitly since this crate doesn't know that default.
pub fn decrypt_with_site_wasm(wasm_bytes: &[u8], ciphertext: &[u8], header_offset: u32) -> Result<String> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm_bytes).map_err(|e| anyhow::anyhow!("{e}")).context("failed to compile site wasm module")?;

    let mut store = Store::new(&engine, ());
    // Matches `WebAssembly.instantiate(module, {})`, the real site module
    // takes no imports, it's a pure compute module.
    let instance =
        Instance::new(&mut store, &module, &[]).map_err(|e| anyhow::anyhow!("{e}")).context("failed to instantiate site wasm module")?;

    let memory = instance
        .get_memory(&mut store, "memory")
        .context("site wasm module does not export memory")?;
    let alloc: wasmtime::TypedFunc<u32, u32> = instance
        .get_typed_func(&mut store, "alloc")
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("site wasm module does not export alloc(len) -> ptr")?;
    let decrypt: wasmtime::TypedFunc<(u32, u32), i32> = instance
        .get_typed_func(&mut store, "decrypt")
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("site wasm module does not export decrypt(ptr, len) -> outLen")?;

    let len = u32::try_from(ciphertext.len()).context("ciphertext too large")?;
    let ptr = alloc.call(&mut store, len).map_err(|e| anyhow::anyhow!("{e}"))?;

    memory
        .write(&mut store, ptr as usize, ciphertext)
        .context("failed to write ciphertext into site wasm memory")?;

    let out_len = decrypt
        .call(&mut store, (ptr, len))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if out_len < 0 {
        anyhow::bail!("site wasm decrypt returned {out_len}");
    }

    let start = ptr as usize + header_offset as usize;
    let mut plaintext = vec![0u8; out_len as usize];
    memory
        .read(&store, start, &mut plaintext)
        .context("failed to read plaintext from site wasm memory")?;

    Ok(String::from_utf8_lossy(&plaintext).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal module matching the real ABI (alloc/decrypt/memory) as an
    /// identity transform, so the test only needs to prove the compile →
    /// instantiate → write → call → read plumbing works, not any particular
    /// site's crypto. `wasmtime::Module::new` accepts WAT text directly.
    const IDENTITY_DECRYPT_WAT: &str = r#"
        (module
          (memory (export "memory") 1)
          (func (export "alloc") (param $len i32) (result i32) (i32.const 0))
          (func (export "decrypt") (param $ptr i32) (param $len i32) (result i32) (local.get $len)))
    "#;

    #[test]
    fn decrypts_via_the_real_alloc_write_call_read_sequence() {
        let plaintext = decrypt_with_site_wasm(IDENTITY_DECRYPT_WAT.as_bytes(), b"hello", 0).unwrap();
        assert_eq!(plaintext, "hello");
    }

    #[test]
    fn rejects_a_module_missing_the_expected_exports() {
        // An empty/invalid module should fail to compile, not panic.
        let result = decrypt_with_site_wasm(&[0, 1, 2, 3], b"anything", 12);
        assert!(result.is_err());
    }
}
