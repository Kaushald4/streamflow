//! Native Viduki WASM bridge, stateful pepper + envelope decryption.
//! Mirrors `stream-resolver/src/primitives/viduki-wasm.ts` (QuickJS has no WebAssembly).

use anyhow::{Context, Result};
use wasmtime::{Engine, Instance, Linker, Module, Store, Val};

pub struct VidukiBridge {
    store: Store<()>,
    instance: Instance,
}

impl VidukiBridge {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self> {
        let engine = Engine::default();
        let module = Module::new(&engine, wasm_bytes)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to compile viduki wasm module")?;

        let mut linker = Linker::new(&engine);
        // makima.wat: (import "env" "abort" (func (param i32 i32 i32 i32)))
        linker
            .func_wrap("env", "abort", |_a: i32, _b: i32, _c: i32, _d: i32| {
                // Only called on internal WASM failure, match browser stub behavior.
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to link env.abort")?;

        let mut store = Store::new(&engine, ());
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to instantiate viduki wasm module")?;
        Ok(Self { store, instance })
    }

    fn memory(&mut self) -> wasmtime::Memory {
        self.instance
            .get_memory(&mut self.store, "memory")
            .expect("viduki wasm exports memory")
    }

    fn alloc(&mut self, size: u32) -> Result<u32> {
        let alloc: wasmtime::TypedFunc<u32, u32> = self
            .instance
            .get_typed_func(&mut self.store, "_NUwd")
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("viduki wasm missing _NUwd (alloc)")?;
        alloc
            .call(&mut self.store, size)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn write(&mut self, ptr: u32, data: &[u8]) -> Result<()> {
        self.memory()
            .write(&mut self.store, ptr as usize, data)
            .context("failed to write viduki wasm memory")
    }

    fn read(&mut self, ptr: u32, len: u32) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len as usize];
        self.memory()
            .read(&self.store, ptr as usize, &mut buf)
            .context("failed to read viduki wasm memory")?;
        Ok(buf)
    }

    pub fn reset(&mut self) -> Result<()> {
        let reset: wasmtime::TypedFunc<(), ()> = self
            .instance
            .get_typed_func(&mut self.store, "_vxNZ")
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("viduki wasm missing _vxNZ (reset)")?;
        reset.call(&mut self.store, ()).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub fn decrypt_pepper(
        &mut self,
        nonce: &[u8],
        bucket: u64,
        iv: &[u8],
        ct: &[u8],
        tag: &[u8],
    ) -> Result<()> {
        let nonce_ptr = self.alloc(u32::try_from(nonce.len())?)?;
        self.write(nonce_ptr, nonce)?;

        let mut bucket_buf = [0u8; 8];
        bucket_buf.copy_from_slice(&bucket.to_be_bytes());
        let bucket_ptr = self.alloc(8)?;
        self.write(bucket_ptr, &bucket_buf)?;

        let iv_ptr = self.alloc(u32::try_from(iv.len())?)?;
        self.write(iv_ptr, iv)?;

        let ct_ptr = self.alloc(u32::try_from(ct.len())?)?;
        self.write(ct_ptr, ct)?;

        let tag_ptr = self.alloc(u32::try_from(tag.len())?)?;
        self.write(tag_ptr, tag)?;

        let decrypt: wasmtime::TypedFunc<
            (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32),
            i32,
        > = self
            .instance
            .get_typed_func(&mut self.store, "_E5Uu")
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("viduki wasm missing _E5Uu (decryptPepper)")?;

        let result = decrypt
            .call(
                &mut self.store,
                (
                    nonce_ptr,
                    nonce.len() as u32,
                    bucket_ptr,
                    8,
                    iv_ptr,
                    iv.len() as u32,
                    ct_ptr,
                    ct.len() as u32,
                    tag_ptr,
                    tag.len() as u32,
                ),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if result == 0 {
            anyhow::bail!("decryptPepper failed (returned 0)");
        }
        Ok(())
    }

    pub fn decrypt_envelope(
        &mut self,
        envelope: &VidukiEnvelope,
        client_nonce_hex: &str,
        request_id_hex: &str,
    ) -> Result<String> {
        let client_nonce = hex_decode(client_nonce_hex)?;
        let server_nonce = hex_decode(&envelope.sn)?;
        let request_id = hex_decode(request_id_hex)?;
        let iv1 = hex_decode(&envelope.iv1)?;
        let iv2 = hex_decode(&envelope.iv2)?;
        let wk = hex_decode(&envelope.wk)?;
        let tag1 = hex_decode(&envelope.tag1)?;
        let tag2 = hex_decode(&envelope.tag2)?;
        let ct = hex_decode(&envelope.ct)?;

        let client_nonce_ptr = self.alloc(u32::try_from(client_nonce.len())?)?;
        self.write(client_nonce_ptr, &client_nonce)?;

        let server_nonce_ptr = self.alloc(u32::try_from(server_nonce.len())?)?;
        self.write(server_nonce_ptr, &server_nonce)?;

        let mut tb_buf = [0u8; 8];
        tb_buf.copy_from_slice(&envelope.tb.to_be_bytes());
        let tb_ptr = self.alloc(8)?;
        self.write(tb_ptr, &tb_buf)?;

        let request_id_ptr = self.alloc(u32::try_from(request_id.len())?)?;
        self.write(request_id_ptr, &request_id)?;

        let iv1_ptr = self.alloc(u32::try_from(iv1.len())?)?;
        self.write(iv1_ptr, &iv1)?;

        let iv2_ptr = self.alloc(u32::try_from(iv2.len())?)?;
        self.write(iv2_ptr, &iv2)?;

        let wk_ptr = self.alloc(u32::try_from(wk.len())?)?;
        self.write(wk_ptr, &wk)?;

        let tag1_ptr = self.alloc(u32::try_from(tag1.len())?)?;
        self.write(tag1_ptr, &tag1)?;

        let tag2_ptr = self.alloc(u32::try_from(tag2.len())?)?;
        self.write(tag2_ptr, &tag2)?;

        let ct_ptr = self.alloc(u32::try_from(ct.len())?)?;
        self.write(ct_ptr, &ct)?;

        let output_ptr = self.alloc(u32::try_from(ct.len())?)?;

        let decrypt = self
            .instance
            .get_func(&mut self.store, "_RHMG")
            .context("viduki wasm missing _RHMG (decryptEnvelope)")?;

        let args = [
            Val::I32(client_nonce_ptr as i32),
            Val::I32(client_nonce.len() as i32),
            Val::I32(server_nonce_ptr as i32),
            Val::I32(server_nonce.len() as i32),
            Val::I32(tb_ptr as i32),
            Val::I32(8),
            Val::I32(request_id_ptr as i32),
            Val::I32(request_id.len() as i32),
            Val::I32(iv2_ptr as i32),
            Val::I32(iv2.len() as i32),
            Val::I32(wk_ptr as i32),
            Val::I32(wk.len() as i32),
            Val::I32(tag2_ptr as i32),
            Val::I32(tag2.len() as i32),
            Val::I32(iv1_ptr as i32),
            Val::I32(iv1.len() as i32),
            Val::I32(ct_ptr as i32),
            Val::I32(ct.len() as i32),
            Val::I32(tag1_ptr as i32),
            Val::I32(tag1.len() as i32),
            Val::I32(output_ptr as i32),
        ];
        let mut results = [Val::I32(0)];
        decrypt
            .call(&mut self.store, &args, &mut results)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let result_len = match results[0] {
            Val::I32(n) => n,
            _ => anyhow::bail!("decryptEnvelope returned unexpected type"),
        };

        if result_len <= 0 {
            anyhow::bail!("decryptEnvelope failed (returned {result_len})");
        }

        let plaintext = self.read(output_ptr, result_len as u32)?;
        Ok(String::from_utf8_lossy(&plaintext).into_owned())
    }

    pub fn drop_pepper(&mut self) -> Result<()> {
        let drop_fn: wasmtime::TypedFunc<(), ()> = self
            .instance
            .get_typed_func(&mut self.store, "_chDt")
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("viduki wasm missing _chDt (dropPepper)")?;
        drop_fn.call(&mut self.store, ()).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

#[derive(Debug, Clone)]
pub struct VidukiEnvelope {
    pub sn: String,
    pub tb: u64,
    pub iv1: String,
    pub iv2: String,
    pub wk: String,
    pub tag1: String,
    pub tag2: String,
    pub ct: String,
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        anyhow::bail!("invalid hex length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("invalid hex"))
        .collect()
}
