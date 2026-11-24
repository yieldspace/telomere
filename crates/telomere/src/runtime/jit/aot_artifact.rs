//! AoT artifact container (#198): signed native-code bundles with an
//! exact-build fingerprint and a fail-closed load path.
//!
//! A native artifact is arbitrary code execution once its bytes are
//! mapped executable, so the trust boundary is cryptographic (who
//! produced this), not structural (what does it look like). The load
//! path therefore runs in a strict order:
//!
//! 1. Parse only the outer envelope (magic, format version, payload
//!    length, signature block lengths).
//! 2. Verify the signature over the whole container with an
//!    embedder-supplied [`AotVerifier`]. Unsigned artifacts are refused
//!    unless the embedder explicitly opts in with
//!    [`AotLoadPolicy::allow_unsigned`]. No section is interpreted and
//!    no executable mapping exists before this step passes: opening an
//!    artifact takes no store and cannot allocate code memory at all.
//! 3. Check the exact-build fingerprint (below). Any field difference
//!    is a typed rejection naming both fingerprints.
//! 4. Check the module binding: the SHA-256 of the embedded
//!    precompiled module (#196) must match the binding recorded in the
//!    signed payload, so native blobs can never be paired with a
//!    different module.
//! 5. Deserialize the precompiled module, then validate the function
//!    table structurally (funcidx range, reloc offsets inside the
//!    blob, stub indices inside the stub table).
//!
//! Only [`install_aot_artifact`] touches a store: it re-checks every
//! function's layout checksum against the materialized form of the
//! instance's actual functions, patches a private copy of each blob
//! (stub addresses from this process, funcaddrs from this instance),
//! and installs the whole batch into the store's JIT cache atomically
//! — any failure installs nothing. There is no silent fallback to
//! parsing wasm anywhere in this path; falling back is the embedder's
//! explicit decision.
//!
//! # Fingerprint
//!
//! Emitted code bakes `repr(Rust)` layout constants that Rust does not
//! guarantee across compilations, so artifacts are pinned to the exact
//! build, not the version. The fingerprint covers: format version,
//! crate version, rustc version, target triple, cargo feature set, the
//! #196 source-tree hash (any change to a layout constant's source
//! changes it mechanically), the op-ID table hash, the stub-table
//! hash, a layout digest (`size_of`/`align_of` of the types whose
//! layout emission reads), and the emission mode string. The mode is
//! `baseline-unmetered-v1`: AoT emission never instruments metering
//! checkpoints, so installation into a metered store is refused.
//!
//! # Position independence (v1 tradeoff)
//!
//! Funcaddr immediates are store-level, so patched code is
//! per-instance, exactly like today's JIT cache. Patching dirties
//! essentially all text pages, so AoT text is anonymous-equivalent
//! memory: the artifact saves load *time*, not resident code memory.
//! A future clean-text mode is deferred (see the issue).
//!
//! # Not a validation substitute
//!
//! Structural checks are defense in depth. Wasm-sandbox guarantees
//! hold only for artifacts produced by a trusted builder from
//! validated wasm; a signature from an untrusted key is worth nothing.
//!
//! # Patching availability
//!
//! Relocation patch encodings live in the emit-gated backends, so the
//! install step is fully functional in `jit` builds (which include
//! `aot-load`). In an `aot-load`-only build, opening/verifying works
//! but [`install_aot_artifact`] fails closed with
//! [`AotError::PatchingUnavailable`].

use std::collections::HashMap;

use crate::common::precompiled::{deserialize_module, serialize_module, PrecompiledError};
use crate::common::sha256::sha256;
use crate::common::store::FunctionBody;
use crate::common::FunctionBody as ModuleFunctionBody;
use crate::common::{ImportDesc, Instr, Module, ObjectRef, Store};
use crate::InstanceHandle;

#[cfg(feature = "jit")]
use super::backend;
#[cfg(feature = "jit")]
use super::emit_env::{ModuleFacts, Reloc, RelocKind};
use super::stubs::{stub_table_fingerprint, StubId};

/// Magic bytes at the start of every AoT artifact container.
const MAGIC: &[u8; 8] = b"TELOAOT1";

/// Container format version written by this build. Bumped on any
/// format change; artifacts with any other version are rejected.
const FORMAT_VERSION: u32 = 1;

/// Emission mode recorded in the fingerprint. AoT emission is the
/// baseline tier without metering instrumentation.
const EMIT_MODE: &str = "baseline-unmetered-v1";

const MAX_CODE_LEN: usize = 1 << 24;
const MAX_FUNCTIONS: usize = 1 << 16;
const MAX_RELOCS: usize = 1 << 20;
const MAX_TRAP_SITES: usize = 1 << 20;

/// Signs artifact payloads. Implementations are embedder-chosen
/// (ed25519 is the suggested default); the container stores whatever
/// opaque signature bytes the signer returns, tagged with its key id.
pub trait AotSigner {
    /// Identifier of the signing key, stored in the container.
    fn key_id(&self) -> &str;
    /// Produces a signature over `payload` (the whole container up to
    /// the signature block).
    fn sign(&self, payload: &[u8]) -> Vec<u8>;
}

/// Verifies artifact signatures, the mirror of [`AotSigner`].
pub trait AotVerifier {
    /// Identifier of the verification key. An artifact signed under a
    /// different key id is rejected before `verify` is called.
    fn key_id(&self) -> &str;
    /// Returns true iff `signature` is valid for `payload`.
    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool;
}

/// Load policy for [`open_aot_artifact`].
#[derive(Debug, Clone, Copy, Default)]
pub struct AotLoadPolicy {
    /// Accept artifacts with an empty signature block. This is the
    /// honest dev-mode opt-in (the CLI spells it
    /// `--trust-unverified-artifact`): anyone who can write the file
    /// can then execute code in the host, so it must never be a
    /// default.
    pub allow_unsigned: bool,
}

/// The exact-build fingerprint carried by an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AotFingerprint {
    /// Container format version.
    pub format_version: u32,
    /// Crate version of the producing build.
    pub crate_version: String,
    /// rustc version of the producing build.
    pub rustc_version: String,
    /// Target triple of the producing build.
    pub target: String,
    /// Cargo feature set of the producing build.
    pub features: String,
    /// #196 source-tree hash of the producing build.
    pub source_hash: String,
    /// Hash of the stable op-ID table.
    pub op_table_hash: u64,
    /// Hash of the runtime stub table (ids and names, in order).
    pub stub_table_hash: u64,
    /// Digest of the layout facts emission depends on.
    pub layout_hash: u64,
    /// Emission mode string (tier + metering posture).
    pub emit_mode: String,
}

impl std::fmt::Display for AotFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "format={} crate={} rustc={} target={} features={} source={} \
             op_table={:#018x} stub_table={:#018x} layout={:#018x} mode={}",
            self.format_version,
            self.crate_version,
            self.rustc_version,
            self.target,
            self.features,
            self.source_hash,
            self.op_table_hash,
            self.stub_table_hash,
            self.layout_hash,
            self.emit_mode,
        )
    }
}

/// Errors returned by the AoT artifact producer and loader. Every
/// failure is one of these typed errors; the decoder never panics.
#[derive(Debug, thiserror::Error)]
pub enum AotError {
    /// The input does not start with the AoT container magic.
    #[error("not a telomere AoT artifact (bad magic)")]
    BadMagic,
    /// The container format version is not the one this build writes.
    #[error("unsupported AoT format version {found} (this build writes {supported})")]
    UnsupportedFormatVersion {
        /// Version found in the artifact.
        found: u32,
        /// Version this build writes.
        supported: u32,
    },
    /// The artifact was signed, but no verifier was supplied.
    #[error("AoT artifact is signed with key '{key_id}' but no verifier was supplied")]
    NoVerifier {
        /// The artifact's signing key id.
        key_id: String,
    },
    /// The artifact is unsigned and the load policy refuses unsigned
    /// artifacts (the default posture).
    #[error("unsigned AoT artifact refused (load policy does not allow unverified artifacts)")]
    UnsignedRefused,
    /// The artifact's key id does not match the verifier's.
    #[error("AoT artifact key id '{artifact}' does not match verifier key id '{verifier}'")]
    KeyIdMismatch {
        /// Key id recorded in the artifact.
        artifact: String,
        /// Key id of the supplied verifier.
        verifier: String,
    },
    /// Signature verification failed.
    #[error("AoT artifact signature verification failed for key '{key_id}'")]
    SignatureInvalid {
        /// The artifact's signing key id.
        key_id: String,
    },
    /// The artifact's build fingerprint differs from this build's.
    #[error(
        "AoT artifact was produced by a different build:\n  artifact: {artifact}\n  this build: {current}"
    )]
    FingerprintMismatch {
        /// The artifact's fingerprint.
        artifact: Box<AotFingerprint>,
        /// This build's fingerprint.
        current: Box<AotFingerprint>,
    },
    /// The embedded module's digest differs from the signed binding.
    #[error("AoT module binding mismatch: native blobs do not belong to the embedded module")]
    ModuleBindingMismatch,
    /// The embedded precompiled module failed to deserialize.
    #[error("embedded precompiled module is invalid: {0}")]
    Precompiled(#[from] PrecompiledError),
    /// The input ended before a complete value could be read.
    #[error("AoT artifact is truncated at offset {offset} (needed {needed} bytes)")]
    Truncated {
        /// Offset at which reading failed.
        offset: usize,
        /// Bytes needed at that offset.
        needed: usize,
    },
    /// A count or length exceeds the format's limits or the remaining
    /// input.
    #[error("AoT artifact declares an impossible {what}: {value}")]
    LimitExceeded {
        /// What was being read.
        what: &'static str,
        /// The declared value.
        value: u64,
    },
    /// Bytes remain after the signature block.
    #[error("AoT artifact has {extra} trailing bytes after the signature block")]
    TrailingBytes {
        /// Number of trailing bytes.
        extra: usize,
    },
    /// A function-table entry names a function the module does not
    /// define.
    #[error("AoT function entry for funcidx {funcidx} but the module has {functions} functions")]
    UnknownFuncidx {
        /// The offending function index.
        funcidx: u32,
        /// Total functions in the module's index space.
        functions: u32,
    },
    /// A relocation offset lies outside its function's blob.
    #[error(
        "AoT relocation offset {offset} is outside the {code_len}-byte blob of function {funcidx}"
    )]
    RelocOutOfBounds {
        /// Function the relocation belongs to.
        funcidx: u32,
        /// The relocation offset.
        offset: u32,
        /// The blob length.
        code_len: u32,
    },
    /// A relocation names a stub index outside the stub table.
    #[error("AoT relocation names stub index {index} but the stub table has {stubs} entries")]
    UnknownStub {
        /// The offending stub index.
        index: u32,
        /// Number of stubs in this build's table.
        stubs: u32,
    },
    /// A function's layout checksum differs from the materialized form
    /// of the function it would be installed for.
    #[error(
        "AoT layout checksum mismatch for function {funcidx} (artifact {artifact:#018x}, actual {actual:#018x})"
    )]
    LayoutChecksumMismatch {
        /// The function whose checksum differs.
        funcidx: u32,
        /// Checksum recorded in the artifact.
        artifact: u64,
        /// Checksum of the actual function.
        actual: u64,
    },
    /// Relocation patching rejected a blob (placeholder verification
    /// failed): the bytes are not the ones the relocations were
    /// recorded against.
    #[error("AoT relocation patching failed for function {funcidx}")]
    RelocPatchFailed {
        /// The function whose blob failed to patch.
        funcidx: u32,
    },
    /// The store has execution metering enabled; AoT code is emitted
    /// unmetered and must never run in a metered store.
    #[error("AoT artifacts cannot be installed into a metered store (AoT code is unmetered)")]
    MeteredStore,
    /// The handle does not belong to the store, or the instance's
    /// function space does not match the artifact's module.
    #[error("AoT install target instance does not match the artifact's module")]
    InstanceMismatch,
    /// The store's code-cache budget cannot hold the batch; nothing
    /// was installed.
    #[error("AoT batch of {needed} bytes exceeds the code-cache budget ({max} bytes)")]
    CodeCacheBudget {
        /// Resident bytes the batch needs.
        needed: usize,
        /// The store's configured budget.
        max: usize,
    },
    /// This build cannot patch relocations (an `aot-load`-only build
    /// has no emit-gated backends); opening still works.
    #[error("this build cannot patch AoT relocations (built without the `jit` feature)")]
    PatchingUnavailable,
    /// A materialized op has no stable op id (producer-side).
    #[error("cannot build AoT artifact: a materialized op has no stable op id")]
    UnregisteredOp,
}

type Result<T> = std::result::Result<T, AotError>;

// ---------------------------------------------------------------------------
// Fingerprint

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn layout_hash() -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for value in [
        std::mem::size_of::<Instr>() as u64,
        std::mem::align_of::<Instr>() as u64,
        std::mem::size_of::<ObjectRef>() as u64,
        std::mem::size_of::<super::JitNativeExit>() as u64,
        std::mem::size_of::<usize>() as u64,
    ] {
        hash = fnv1a(hash, &value.to_le_bytes());
    }
    fnv1a(hash, EMIT_MODE.as_bytes())
}

/// This build's fingerprint.
pub fn current_fingerprint() -> AotFingerprint {
    AotFingerprint {
        format_version: FORMAT_VERSION,
        crate_version: env!("CARGO_PKG_VERSION").to_string(),
        rustc_version: env!("TELOMERE_BUILD_RUSTC_VERSION").to_string(),
        target: env!("TELOMERE_BUILD_TARGET").to_string(),
        features: crate::common::precompiled::feature_set_string(),
        source_hash: env!("TELOMERE_BUILD_SOURCE_HASH").to_string(),
        op_table_hash: crate::common::op_registry::OP_TABLE_HASH,
        stub_table_hash: stub_table_fingerprint(),
        layout_hash: layout_hash(),
        emit_mode: EMIT_MODE.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Byte writer / reader (canonical LEB128, bounds-checked, total)

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }
    fn raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }
    fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }
    fn leb_u64(&mut self, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.buf.push(byte);
            if value == 0 {
                break;
            }
        }
    }
    fn leb_u32(&mut self, value: u32) {
        self.leb_u64(u64::from(value));
    }
    fn fixed_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }
    fn fixed_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }
    fn string(&mut self, value: &str) {
        self.leb_u64(value.len() as u64);
        self.raw(value.as_bytes());
    }
    fn bytes(&mut self, value: &[u8]) {
        self.leb_u64(value.len() as u64);
        self.raw(value);
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.remaining() < len {
            return Err(AotError::Truncated {
                offset: self.pos,
                needed: len,
            });
        }
        let out = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn leb_u64(&mut self) -> Result<u64> {
        let start = self.pos;
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            if shift == 63 && byte > 1 {
                return Err(AotError::LimitExceeded {
                    what: "LEB128 value",
                    value: u64::MAX,
                });
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                return Err(AotError::LimitExceeded {
                    what: "LEB128 value",
                    value: u64::MAX,
                });
            }
        }
        // Canonical (minimal) encoding only: re-encode and compare.
        let mut check = Writer::new();
        check.leb_u64(value);
        if check.buf.len() != self.pos - start {
            return Err(AotError::LimitExceeded {
                what: "non-canonical LEB128 value",
                value,
            });
        }
        Ok(value)
    }
    fn leb_u32(&mut self) -> Result<u32> {
        let value = self.leb_u64()?;
        u32::try_from(value).map_err(|_| AotError::LimitExceeded {
            what: "u32 value",
            value,
        })
    }
    fn count(&mut self, what: &'static str, max: usize, min_bytes_each: usize) -> Result<usize> {
        let value = self.leb_u64()?;
        let count = usize::try_from(value).map_err(|_| AotError::LimitExceeded { what, value })?;
        if count > max || count > self.remaining() / min_bytes_each.max(1) {
            return Err(AotError::LimitExceeded { what, value });
        }
        Ok(count)
    }
    fn fixed_u64(&mut self) -> Result<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().expect("8 bytes")))
    }
    fn fixed_u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
    }
    fn string(&mut self) -> Result<String> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes.to_vec()).map_err(|_| AotError::LimitExceeded {
            what: "UTF-8 string",
            value: bytes.len() as u64,
        })
    }
    fn bytes(&mut self) -> Result<&'a [u8]> {
        let len = self.leb_u64()?;
        let len = usize::try_from(len).map_err(|_| AotError::LimitExceeded {
            what: "byte string length",
            value: len,
        })?;
        self.take(len)
    }
}

// ---------------------------------------------------------------------------
// Function entries

/// The kind of an [`AotReloc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AotRelocKind {
    /// Patch with the address of the stub at this index in the
    /// build's stub table ([`StubId::ALL`] order).
    Stub(u32),
    /// Patch with the funcaddr of this index in the module's function
    /// index space, resolved against the installing instance.
    FuncAddr(u32),
}

/// One relocation inside a function's native blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AotReloc {
    /// Byte offset of the placeholder immediate in the blob.
    pub offset: u32,
    /// What the site must be patched with.
    pub kind: AotRelocKind,
}

/// One function's native blob in an opened artifact.
#[derive(Debug, Clone)]
pub struct AotFunction {
    funcidx: u32,
    layout_checksum: u64,
    trap_sites: Vec<u32>,
    code: Vec<u8>,
    relocs: Vec<AotReloc>,
}

impl AotFunction {
    /// Index in the module's function index space.
    pub fn funcidx(&self) -> u32 {
        self.funcidx
    }
    /// Native code length in bytes (with placeholder immediates).
    pub fn code_len(&self) -> usize {
        self.code.len()
    }
    /// Number of recorded relocations.
    pub fn reloc_count(&self) -> usize {
        self.relocs.len()
    }
    /// Number of recorded #209 trap sites.
    pub fn trap_site_count(&self) -> usize {
        self.trap_sites.len()
    }
}

// ---------------------------------------------------------------------------
// Layout checksum

fn op_id_map() -> HashMap<usize, u16> {
    let mut map = HashMap::new();
    crate::common::op_registry::for_each_op_entry(|entry| {
        map.insert(entry.op as usize, entry.id);
    });
    map
}

/// FNV-1a over the lowered form (op ids, labels, operands, const
/// pool, jump table, block map) plus the materialized stream's op
/// lengths — the materialized form is a pure function of the lowered
/// form, so this pins the exact stream the emission plan was built
/// against. Any materialization divergence (a different optimizer
/// output, a mode-dependent rewrite) changes this value, so a blob
/// is never installed for a function whose form differs from the one
/// it was emitted from.
fn layout_checksum(
    lowered: &crate::common::LoweredFunction,
    op_ids: &HashMap<usize, u16>,
) -> Result<u64> {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mix_u64 = |hash: &mut u64, value: u64| {
        *hash = fnv1a(*hash, &value.to_le_bytes());
    };
    for op in &lowered.code {
        let id = op_ids
            .get(&(op.op as usize))
            .ok_or(AotError::UnregisteredOp)?;
        mix_u64(&mut hash, u64::from(*id));
        mix_u64(&mut hash, op.label.map(|l| l as u64 + 1).unwrap_or(0));
        mix_u64(&mut hash, op.operands.len() as u64);
        for operand in &op.operands {
            match operand {
                crate::common::LoweredOperand::Raw(bytes) => {
                    mix_u64(&mut hash, 0);
                    hash = fnv1a(hash, bytes);
                }
                crate::common::LoweredOperand::ConstPoolRef(index) => {
                    mix_u64(&mut hash, 1);
                    mix_u64(&mut hash, u64::from(*index));
                }
                crate::common::LoweredOperand::JumpTarget(index) => {
                    mix_u64(&mut hash, 2);
                    mix_u64(&mut hash, *index as u64);
                }
                crate::common::LoweredOperand::CallRecipeRef(recipe) => {
                    mix_u64(&mut hash, 3);
                    mix_u64(&mut hash, u64::from(recipe.funcidx));
                }
            }
        }
    }
    for entry in &lowered.const_pool {
        hash = fnv1a(hash, entry);
    }
    for target in &lowered.jump_table {
        mix_u64(&mut hash, target.label as u64);
        mix_u64(&mut hash, target.block_id as u64);
    }
    for block in &lowered.block_map {
        mix_u64(&mut hash, block.block_id as u64);
        mix_u64(&mut hash, block.label as u64);
        mix_u64(&mut hash, block.code_index as u64);
    }
    // The materialized stream's shape (op count and per-op lengths)
    // is what the emission plan was built against; it is a pure
    // function of the lowered form hashed above.
    let materialized = lowered.materialize();
    mix_u64(&mut hash, materialized.instrs.len() as u64);
    for len in &materialized.op_lens {
        mix_u64(&mut hash, u64::from(*len));
    }
    Ok(hash)
}

fn imported_func_count(module: &Module) -> u32 {
    module
        .imports
        .0
        .iter()
        .filter(|import| matches!(import.desc, ImportDesc::TypeIdx(_)))
        .count() as u32
}

// ---------------------------------------------------------------------------
// Producer

/// Builds an AoT artifact for `module`: embeds the #196 precompiled
/// serialization, AoT-emits every defined function the baseline
/// emitter accepts (functions it rejects simply get no blob and run
/// interpreted, which per the JIT coverage audit is the normal case
/// for bridge-heavy code), and signs the container with `signer` —
/// or leaves it unsigned when `signer` is `None` (dev mode; loaders
/// refuse such artifacts unless explicitly configured otherwise).
#[cfg(feature = "jit")]
pub fn build_aot_artifact(module: &Module, signer: Option<&dyn AotSigner>) -> Result<Vec<u8>> {
    let precompiled = serialize_module(module)?;
    let binding = sha256(&precompiled);
    let op_ids = op_id_map();
    let imported = imported_func_count(module);

    let mut functions = Writer::new();
    let mut function_count: u32 = 0;
    for (index, body) in module.codes.0.iter().enumerate() {
        let ModuleFunctionBody::Wasm(func) = body else {
            continue;
        };
        let lowered = &func.lowered;
        let funcidx = imported + index as u32;
        let materialized = lowered.materialize();
        let emission = match backend::emit_baseline_function_aot(
            &materialized.instrs,
            &materialized.op_lens,
            ModuleFacts::new(module),
        ) {
            Ok(emission) => emission,
            Err(_) => continue, // JIT-rejected: no blob, interpreted.
        };
        let checksum = layout_checksum(lowered, &op_ids)?;
        functions.leb_u32(funcidx);
        functions.fixed_u64(checksum);
        functions.leb_u64(emission.trap_sites.len() as u64);
        for site in &emission.trap_sites {
            functions.fixed_u32(*site);
        }
        functions.bytes(&emission.bytes);
        functions.leb_u64(emission.relocs.len() as u64);
        for reloc in &emission.relocs {
            functions.leb_u32(reloc.offset);
            match reloc.kind {
                RelocKind::Stub(id) => {
                    let index = StubId::ALL
                        .iter()
                        .position(|entry| *entry as u32 == id as u32)
                        .ok_or(AotError::UnregisteredOp)?;
                    functions.u8(0);
                    functions.leb_u32(index as u32);
                }
                RelocKind::FuncAddr(funcidx) => {
                    functions.u8(1);
                    functions.leb_u32(funcidx);
                }
            }
        }
        function_count += 1;
    }

    let fingerprint = current_fingerprint();
    let mut payload = Writer::new();
    write_fingerprint(&mut payload, &fingerprint);
    payload.raw(&binding);
    payload.bytes(&precompiled);
    payload.leb_u32(function_count);
    payload.raw(&functions.buf);

    let mut out = Writer::new();
    out.raw(MAGIC);
    out.leb_u32(FORMAT_VERSION);
    out.leb_u64(payload.buf.len() as u64);
    out.raw(&payload.buf);
    match signer {
        Some(signer) => {
            // Sign the container-so-far (magic, version, payload)
            // before the signature block itself is appended; the
            // loader verifies exactly that region.
            let signature = signer.sign(&out.buf);
            out.string(signer.key_id());
            out.bytes(&signature);
        }
        None => {
            out.string("");
            out.bytes(&[]);
        }
    }
    Ok(out.buf)
}

fn write_fingerprint(w: &mut Writer, fingerprint: &AotFingerprint) {
    w.string(&fingerprint.crate_version);
    w.string(&fingerprint.rustc_version);
    w.string(&fingerprint.target);
    w.string(&fingerprint.features);
    w.string(&fingerprint.source_hash);
    w.fixed_u64(fingerprint.op_table_hash);
    w.fixed_u64(fingerprint.stub_table_hash);
    w.fixed_u64(fingerprint.layout_hash);
    w.string(&fingerprint.emit_mode);
}

fn read_fingerprint(r: &mut Reader<'_>, format_version: u32) -> Result<AotFingerprint> {
    Ok(AotFingerprint {
        format_version,
        crate_version: r.string()?,
        rustc_version: r.string()?,
        target: r.string()?,
        features: r.string()?,
        source_hash: r.string()?,
        op_table_hash: r.fixed_u64()?,
        stub_table_hash: r.fixed_u64()?,
        layout_hash: r.fixed_u64()?,
        emit_mode: r.string()?,
    })
}

// ---------------------------------------------------------------------------
// Loader

/// Returns true when `bytes` starts with the AoT container magic.
pub fn is_aot_artifact(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// An opened (verified, fingerprint-checked, deserialized) AoT
/// artifact. Opening performs no store interaction and allocates no
/// executable memory; use [`install_aot_artifact`] to install it.
pub struct AotArtifact {
    fingerprint: AotFingerprint,
    key_id: String,
    unsigned: bool,
    module: Module,
    functions: Vec<AotFunction>,
}

impl std::fmt::Debug for AotArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AotArtifact")
            .field("fingerprint", &self.fingerprint)
            .field("key_id", &self.key_id)
            .field("unsigned", &self.unsigned)
            .field("functions", &self.functions.len())
            .finish_non_exhaustive()
    }
}

impl AotArtifact {
    /// The artifact's build fingerprint (equal to this build's, or
    /// opening would have failed).
    pub fn fingerprint(&self) -> &AotFingerprint {
        &self.fingerprint
    }
    /// The signing key id, or empty for an unsigned artifact.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
    /// Whether the artifact carried no signature (accepted only
    /// under an explicit opt-in policy).
    pub fn is_unsigned(&self) -> bool {
        self.unsigned
    }
    /// The deserialized precompiled module. Instantiate this module,
    /// then install the artifact into that instance.
    pub fn module(&self) -> &Module {
        &self.module
    }
    /// The function blobs carried by the artifact.
    pub fn functions(&self) -> &[AotFunction] {
        &self.functions
    }
}

/// Opens an AoT artifact: verifies the signature before interpreting
/// anything past the outer envelope, checks the exact-build
/// fingerprint, checks the module binding, deserializes the embedded
/// module, and structurally validates the function table. Every
/// failure is a typed [`AotError`]; nothing is partially applied.
pub fn open_aot_artifact(
    bytes: &[u8],
    verifier: Option<&dyn AotVerifier>,
    policy: AotLoadPolicy,
) -> Result<AotArtifact> {
    // Envelope only: magic, version, payload length, signature block.
    if !bytes.starts_with(MAGIC) {
        return Err(AotError::BadMagic);
    }
    let mut envelope = Reader::new(&bytes[MAGIC.len()..]);
    let format_version = envelope.leb_u32()?;
    if format_version != FORMAT_VERSION {
        return Err(AotError::UnsupportedFormatVersion {
            found: format_version,
            supported: FORMAT_VERSION,
        });
    }
    let payload_len = envelope.leb_u64()?;
    let payload_len = usize::try_from(payload_len).map_err(|_| AotError::LimitExceeded {
        what: "payload length",
        value: payload_len,
    })?;
    let payload_start = MAGIC.len() + envelope.pos;
    let payload = envelope.take(payload_len)?;
    let signed_region = &bytes[..payload_start + payload_len];
    let key_id = envelope.string()?;
    let signature = envelope.bytes()?.to_vec();
    if envelope.remaining() != 0 {
        return Err(AotError::TrailingBytes {
            extra: envelope.remaining(),
        });
    }

    // Signature verification, before the payload is interpreted.
    let unsigned = key_id.is_empty() && signature.is_empty();
    if unsigned {
        if !policy.allow_unsigned {
            return Err(AotError::UnsignedRefused);
        }
    } else {
        let verifier = verifier.ok_or_else(|| AotError::NoVerifier {
            key_id: key_id.clone(),
        })?;
        if verifier.key_id() != key_id {
            return Err(AotError::KeyIdMismatch {
                artifact: key_id.clone(),
                verifier: verifier.key_id().to_string(),
            });
        }
        if !verifier.verify(signed_region, &signature) {
            return Err(AotError::SignatureInvalid { key_id });
        }
    }

    // Payload: fingerprint, binding, module, function table.
    let mut r = Reader::new(payload);
    let fingerprint = read_fingerprint(&mut r, format_version)?;
    let current = current_fingerprint();
    if fingerprint != current {
        return Err(AotError::FingerprintMismatch {
            artifact: Box::new(fingerprint),
            current: Box::new(current),
        });
    }
    let binding = r.take(32)?;
    let precompiled = r.bytes()?;
    if sha256(precompiled) != binding {
        return Err(AotError::ModuleBindingMismatch);
    }
    let module = deserialize_module(precompiled)?;
    let total_functions = module.functions.len() as u32;
    let defined_functions = module.codes.0.len() as u32;
    let imported = imported_func_count(&module);
    let op_ids = op_id_map();

    let function_count = r.count("function count", MAX_FUNCTIONS, 9)?;
    let mut functions = Vec::with_capacity(function_count);
    let mut previous_funcidx: Option<u32> = None;
    for _ in 0..function_count {
        let funcidx = r.leb_u32()?;
        if funcidx < imported || funcidx >= imported + defined_functions {
            return Err(AotError::UnknownFuncidx {
                funcidx,
                functions: total_functions,
            });
        }
        if previous_funcidx.is_some_and(|prev| funcidx <= prev) {
            return Err(AotError::LimitExceeded {
                what: "function entries not strictly increasing",
                value: u64::from(funcidx),
            });
        }
        previous_funcidx = Some(funcidx);
        let checksum = r.fixed_u64()?;
        // The checksum is verified now, against the deserialized
        // module, so a tampered or divergent blob is rejected at open
        // time rather than at install time.
        let body = &module.codes.0[(funcidx - imported) as usize];
        let ModuleFunctionBody::Wasm(func) = body else {
            return Err(AotError::UnknownFuncidx {
                funcidx,
                functions: total_functions,
            });
        };
        let actual = layout_checksum(&func.lowered, &op_ids)?;
        if actual != checksum {
            return Err(AotError::LayoutChecksumMismatch {
                funcidx,
                artifact: checksum,
                actual,
            });
        }
        let trap_count = r.count("trap site count", MAX_TRAP_SITES, 4)?;
        let mut trap_sites = Vec::with_capacity(trap_count);
        for _ in 0..trap_count {
            trap_sites.push(r.fixed_u32()?);
        }
        let code = r.bytes()?;
        if code.len() > MAX_CODE_LEN {
            return Err(AotError::LimitExceeded {
                what: "function code length",
                value: code.len() as u64,
            });
        }
        let code = code.to_vec();
        let reloc_count = r.count("reloc count", MAX_RELOCS, 2)?;
        let mut relocs = Vec::with_capacity(reloc_count);
        for _ in 0..reloc_count {
            let offset = r.leb_u32()?;
            if offset as usize >= code.len() {
                return Err(AotError::RelocOutOfBounds {
                    funcidx,
                    offset,
                    code_len: code.len() as u32,
                });
            }
            let kind = match r.u8()? {
                0 => {
                    let index = r.leb_u32()?;
                    if index as usize >= StubId::ALL.len() {
                        return Err(AotError::UnknownStub {
                            index,
                            stubs: StubId::ALL.len() as u32,
                        });
                    }
                    AotRelocKind::Stub(index)
                }
                1 => {
                    let target = r.leb_u32()?;
                    if target >= total_functions {
                        return Err(AotError::UnknownFuncidx {
                            funcidx: target,
                            functions: total_functions,
                        });
                    }
                    AotRelocKind::FuncAddr(target)
                }
                other => {
                    return Err(AotError::LimitExceeded {
                        what: "reloc kind tag",
                        value: u64::from(other),
                    })
                }
            };
            relocs.push(AotReloc { offset, kind });
        }
        functions.push(AotFunction {
            funcidx,
            layout_checksum: checksum,
            trap_sites,
            code,
            relocs,
        });
    }
    if r.remaining() != 0 {
        return Err(AotError::TrailingBytes {
            extra: r.remaining(),
        });
    }

    Ok(AotArtifact {
        fingerprint,
        key_id,
        unsigned,
        module,
        functions,
    })
}

// ---------------------------------------------------------------------------
// Install

/// The outcome of a successful [`install_aot_artifact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AotInstallReport {
    /// Functions whose native blobs were installed into the cache.
    pub installed: usize,
    /// Defined functions with no blob; they run interpreted.
    pub interpreted: usize,
}

/// Installs an opened artifact's native blobs into `store`'s JIT
/// cache for `instance` (which must be an instance of the artifact's
/// module). Every blob is re-validated against the instance's actual
/// functions, patched as a private per-instance copy, and the batch
/// is installed atomically: any failure installs nothing and leaves
/// the cache untouched.
pub fn install_aot_artifact(
    store: &Store,
    instance: &InstanceHandle,
    artifact: &AotArtifact,
) -> Result<AotInstallReport> {
    if store.metering_ref().is_some() {
        return Err(AotError::MeteredStore);
    }
    let instance_addr = instance
        .object_ref_for_store(store)
        .ok_or(AotError::InstanceMismatch)?;
    let runtime = store.lock_runtime_or_panic();
    let instance_data = runtime.get_instance(instance_addr);
    if instance_data.funcs.len() != artifact.module.functions.len() {
        return Err(AotError::InstanceMismatch);
    }
    let op_ids = op_id_map();
    let funcs = instance_data.funcs.clone();
    drop(runtime);

    // Validate + patch everything before touching the cache.
    let mut batch: Vec<(ObjectRef, Vec<u8>, Vec<u32>)> =
        Vec::with_capacity(artifact.functions.len());
    for function in &artifact.functions {
        let funcaddr = funcs
            .get(function.funcidx as usize)
            .copied()
            .ok_or(AotError::InstanceMismatch)?;
        let runtime = store.lock_runtime_or_panic();
        let func = runtime.get_func(funcaddr);
        let FunctionBody::Wasm { lowered, .. } = &func.body else {
            return Err(AotError::InstanceMismatch);
        };
        let actual = layout_checksum(lowered, &op_ids)?;
        if actual != function.layout_checksum {
            return Err(AotError::LayoutChecksumMismatch {
                funcidx: function.funcidx,
                artifact: function.layout_checksum,
                actual,
            });
        }
        drop(runtime);

        let mut code = function.code.clone();
        patch_function_code(&mut code, function, &funcs)?;
        batch.push((funcaddr, code, function.trap_sites.clone()));
    }

    let max_bytes = store.runtime_config().limits.max_code_cache_bytes as usize;
    store
        .jit_cache()
        .install_batch(&batch, max_bytes)
        .map_err(|needed| AotError::CodeCacheBudget {
            needed,
            max: max_bytes,
        })?;
    Ok(AotInstallReport {
        installed: batch.len(),
        interpreted: artifact.module.codes.0.len() - batch.len(),
    })
}

#[cfg(feature = "jit")]
fn patch_function_code(code: &mut [u8], function: &AotFunction, funcs: &[ObjectRef]) -> Result<()> {
    let relocs: Vec<Reloc> = function
        .relocs
        .iter()
        .map(|reloc| Reloc {
            offset: reloc.offset,
            kind: match reloc.kind {
                AotRelocKind::Stub(index) => RelocKind::Stub(StubId::ALL[index as usize]),
                AotRelocKind::FuncAddr(funcidx) => RelocKind::FuncAddr(funcidx),
            },
        })
        .collect();
    backend::patch_relocations(code, &relocs, &|kind| match kind {
        RelocKind::Stub(id) => id.address() as u64,
        RelocKind::FuncAddr(funcidx) => u64::from(funcs[funcidx as usize].get()),
    })
    .map_err(|_| AotError::RelocPatchFailed {
        funcidx: function.funcidx,
    })
}

#[cfg(not(feature = "jit"))]
fn patch_function_code(
    _code: &mut [u8],
    _function: &AotFunction,
    _funcs: &[ObjectRef],
) -> Result<()> {
    Err(AotError::PatchingUnavailable)
}
