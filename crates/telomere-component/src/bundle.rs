//! Component bundles (#201): a component binary plus hash-keyed AoT
//! artifacts for the core modules embedded in it.
//!
//! The product surface is WASI 0.2 components, but the AoT pipeline
//! (#196 codec, #198 container) handles bare core modules. A bundle is
//! the minimal component integration: the component binary stays the
//! source of truth for everything component-level (types, canonical
//! options, the instance graph — none of it is serialized), and each
//! embedded core module additionally carries the exact standalone core
//! AoT artifact (#198 format, byte-identical to a bare-module artifact)
//! keyed by `(section ordinal, SHA-256 of the section's exact bytes)`.
//!
//! * The ordinal is the CoreModule section's index in depth-first
//!   encounter order over the component (nested components included),
//!   which is the order the component decoder registers defined core
//!   modules in. Two byte-identical modules embedded twice are distinct
//!   instances in the component index space, so the key includes the
//!   ordinal, not just the hash.
//! * One signature covers the whole bundle (component binary + the
//!   keyed artifact set), reusing the #198 signer/verifier traits, so
//!   component bytes and native code cannot be mixed and matched. The
//!   inner artifacts are unsigned by construction: the bundle signature
//!   is the single trust root for the set, and the loader opens inner
//!   artifacts only after the bundle-level posture below has passed.
//!
//! # Load path (fail-closed, in order)
//!
//! 1. Parse only the outer envelope (magic, format version, field
//!    lengths, signature block lengths). Nothing is interpreted and no
//!    executable memory exists before step 2 passes — opening a bundle
//!    takes no store.
//! 2. Verify the bundle signature. Unsigned bundles are refused unless
//!    the embedder explicitly opts in with
//!    [`AotLoadPolicy::allow_unsigned`] (the dev-mode posture #198
//!    established; the CLI spells it `--trust-unverified-artifact`).
//! 3. Enumerate the embedded component's CoreModule sections and check
//!    the keyed set against them *before* decoding: a section with no
//!    artifact is [`BundleError::MissingArtifact`], a section whose
//!    bytes hash differently than its key is
//!    [`BundleError::HashMismatch`], an artifact keyed to a section the
//!    component does not have is [`BundleError::ExtraneousArtifact`].
//!    A hash mismatch is never silently re-parsed: the embedder decides
//!    whether running unsigned, re-parsed code is acceptable for its
//!    deployment by choosing to load the bare component instead.
//! 4. Open each inner artifact through the #198 loader (signature
//!    posture already decided at the bundle level; fingerprint and
//!    module-binding checks still run per artifact).
//! 5. Decode the component exactly as [`ComponentEngine::compile`]
//!    does, except each CoreModule section substitutes the opened
//!    artifact's deserialized module for `WasmParser::parse_module`.
//!    The substitution is total: a section the pre-check passed always
//!    has an artifact waiting, and the decoder fails closed
//!    (`ComponentParseError::BundleSubstitutionMissing`) rather than
//!    falling back to parsing if that invariant is ever broken.
//! 6. At instantiation, each defined core instance created from a
//!    substituted module gets its artifact installed with
//!    [`install_aot_artifact`], exactly as in the core path.
//!
//! # What a bundle does not do
//!
//! No component-level structure is serialized (see above), lift/lower
//! canonical-ABI code is not generated, and component semantics under
//! #117/#118/#167 are untouched: the substituted module is the same
//! `Module` value the parser would have produced, validated by the
//! same decoder contract checks.
//!
//! [`ComponentEngine::compile`]: crate::ComponentEngine::compile

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sha2::{Digest, Sha256};
use telomere::{
    AotArtifact, AotError, AotInstallReport, AotLoadPolicy, AotSigner, AotVerifier,
    IoReadBinaryReader, Module, WasmParser,
};

use crate::decoder::ComponentParseError;
use crate::ir::{CoreModule, GlobalIdx};
use crate::support::binary::BinaryReader;
use crate::support::common::InstanceHandle;
use crate::support::Store;
use crate::{ComponentError, ComponentProgram};

/// Magic bytes at the start of every component bundle container.
pub const BUNDLE_MAGIC: &[u8; 8] = b"TELOBND1";

/// Container format version written by this build. Bumped on any
/// format change; bundles with any other version are rejected.
pub const BUNDLE_FORMAT_VERSION: u32 = 1;

/// Upper bound on embedded core modules per bundle (defensive limit,
/// mirroring the #198 container's function-table cap).
const MAX_CORE_MODULES: u64 = 1 << 16;

/// Section id of a core module section in a component binary.
const SECTION_CORE_MODULE: u8 = 0x01;
/// Section id of a nested component section in a component binary.
const SECTION_COMPONENT: u8 = 0x04;

/// Errors returned by the component bundle producer and loader. Every
/// failure is one of these typed errors; the decoder never panics.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// The input does not start with the bundle container magic.
    #[error("not a telomere component bundle (bad magic)")]
    BadMagic,
    /// The container format version is not the one this build writes.
    #[error("unsupported component bundle format version {found} (this build writes {supported})")]
    UnsupportedFormatVersion {
        /// Version found in the bundle.
        found: u32,
        /// Version this build writes.
        supported: u32,
    },
    /// The input ended before a complete value could be read.
    #[error("component bundle is truncated at offset {offset} (needed {needed} bytes)")]
    Truncated {
        /// Offset at which reading failed.
        offset: usize,
        /// Bytes needed at that offset.
        needed: usize,
    },
    /// A count or length exceeds the format's limits or the remaining
    /// input.
    #[error("component bundle declares an impossible {what}: {value}")]
    LimitExceeded {
        /// What was being read.
        what: &'static str,
        /// The declared value.
        value: u64,
    },
    /// Bytes remain after the signature block.
    #[error("component bundle has {extra} trailing bytes after the signature block")]
    TrailingBytes {
        /// Number of trailing bytes.
        extra: usize,
    },
    /// The bundle was signed, but no verifier was supplied.
    #[error("component bundle is signed with key '{key_id}' but no verifier was supplied")]
    NoVerifier {
        /// The bundle's signing key id.
        key_id: String,
    },
    /// The bundle is unsigned and the load policy refuses unsigned
    /// bundles (the default posture).
    #[error("unsigned component bundle refused (load policy does not allow unverified artifacts)")]
    UnsignedRefused,
    /// The bundle's key id does not match the verifier's.
    #[error("component bundle key id '{bundle}' does not match verifier key id '{verifier}'")]
    KeyIdMismatch {
        /// Key id recorded in the bundle.
        bundle: String,
        /// Key id of the supplied verifier.
        verifier: String,
    },
    /// Signature verification failed.
    #[error("component bundle signature verification failed for key '{key_id}'")]
    SignatureInvalid {
        /// The bundle's signing key id.
        key_id: String,
    },
    /// A CoreModule section of the embedded component has no artifact
    /// in the bundle.
    #[error("component bundle holds no artifact for core module section {ordinal}")]
    MissingArtifact {
        /// Ordinal of the CoreModule section without an artifact.
        ordinal: u32,
    },
    /// The artifact keyed to a CoreModule section was produced from
    /// different section bytes than the ones in this component.
    #[error(
        "component bundle artifact for core module section {ordinal} is keyed to hash {expected}, \
         but the section bytes hash to {actual}"
    )]
    HashMismatch {
        /// Ordinal of the mismatched CoreModule section.
        ordinal: u32,
        /// Hash recorded in the bundle entry (hex).
        expected: String,
        /// Hash of the section bytes in the component (hex).
        actual: String,
    },
    /// The bundle holds an artifact for a CoreModule section the
    /// embedded component does not have.
    #[error(
        "component bundle holds an artifact for core module section {ordinal}, \
         but the component has no such section"
    )]
    ExtraneousArtifact {
        /// Ordinal of the artifact without a matching section.
        ordinal: u32,
    },
    /// The embedded component binary is structurally malformed at the
    /// section-walking level (the full decoder reports its own, more
    /// specific error for anything the walker accepts).
    #[error("component in bundle is malformed: {0}")]
    MalformedComponent(&'static str),
    /// Producer side: an embedded core module failed to parse.
    #[error("core module section {ordinal} failed to parse: {source}")]
    CoreModuleParse {
        /// Ordinal of the section that failed to parse.
        ordinal: u32,
        /// The core parser's error.
        #[source]
        source: telomere::WasmParserError,
    },
    /// Producer side: building the core AoT artifact for a section
    /// failed.
    #[error("core module section {ordinal} failed to build an AoT artifact: {source}")]
    CoreModuleBuild {
        /// Ordinal of the section whose artifact failed to build.
        ordinal: u32,
        /// The artifact producer's error.
        #[source]
        source: AotError,
    },
    /// An inner AoT artifact failed the #198 loader (fingerprint,
    /// module binding, or structural validation).
    #[error("AoT artifact for core module section {ordinal} failed to open: {source}")]
    Artifact {
        /// Ordinal of the section whose artifact failed to open.
        ordinal: u32,
        /// The artifact loader's error.
        #[source]
        source: AotError,
    },
    /// Decoding the embedded component failed.
    #[error("component bundle decode failed: {0}")]
    Component(#[from] ComponentError),
}

/// Returns true when `bytes` starts with the bundle container magic.
pub fn is_component_bundle(bytes: &[u8]) -> bool {
    bytes.starts_with(BUNDLE_MAGIC)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// Component section walking
// ---------------------------------------------------------------------------

/// One CoreModule section of a component binary, located by the
/// section walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreModuleSection {
    /// Depth-first encounter ordinal of this section (see module docs).
    pub ordinal: u32,
    /// Byte range of the section payload (the exact core module bytes)
    /// within the component binary.
    pub range: std::ops::Range<usize>,
}

/// Enumerates the CoreModule sections of a component binary in
/// depth-first encounter order, without decoding anything else.
///
/// This is the same traversal the component decoder performs, so the
/// ordinal assigned here is the ordinal the decoder will substitute
/// against. The walker is deliberately shallow: it checks the header
/// magic and section framing only, and leaves all validation to the
/// real decoder.
pub fn enumerate_core_module_sections(
    component: &[u8],
) -> Result<Vec<CoreModuleSection>, BundleError> {
    let mut sections = Vec::new();
    walk_component(component, 0, &mut sections)?;
    Ok(sections)
}

fn walk_component(
    bytes: &[u8],
    depth: u32,
    out: &mut Vec<CoreModuleSection>,
) -> Result<(), BundleError> {
    walk_component_at(bytes, 0, depth, out)
}

/// `base` is the offset of `bytes` within the whole component binary;
/// recorded section ranges are always relative to the whole binary,
/// including for sections inside nested components.
fn walk_component_at(
    bytes: &[u8],
    base: usize,
    depth: u32,
    out: &mut Vec<CoreModuleSection>,
) -> Result<(), BundleError> {
    if depth > crate::MAX_COMPONENT_NESTING_DEPTH {
        // The decoder rejects such components outright; the walker
        // simply does not descend further, so sections below the limit
        // get no ordinal and a bundle built for them cannot substitute
        // (the decode error surfaces instead).
        return Ok(());
    }
    if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
        return Err(BundleError::MalformedComponent(
            "bad component magic or truncated header",
        ));
    }
    let mut pos = 8usize;
    while pos < bytes.len() {
        let id = bytes[pos];
        pos += 1;
        let (size, consumed) = read_leb_u32(&bytes[pos..])?;
        pos += consumed;
        let size = size as usize;
        let end = pos
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or(BundleError::Truncated {
                offset: pos,
                needed: size,
            })?;
        match id {
            SECTION_CORE_MODULE => {
                if out.len() as u64 >= MAX_CORE_MODULES {
                    return Err(BundleError::LimitExceeded {
                        what: "core module section count",
                        value: out.len() as u64 + 1,
                    });
                }
                out.push(CoreModuleSection {
                    ordinal: out.len() as u32,
                    range: base + pos..base + end,
                });
            }
            SECTION_COMPONENT => walk_component_at(&bytes[pos..end], base + pos, depth + 1, out)?,
            _ => {}
        }
        pos = end;
    }
    Ok(())
}

/// Reads an unsigned LEB128 u32 (at most 5 bytes) from the front of
/// `bytes`, returning the value and the number of bytes consumed.
fn read_leb_u32(bytes: &[u8]) -> Result<(u32, usize), BundleError> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for (index, byte) in bytes.iter().enumerate().take(5) {
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            if value > u64::from(u32::MAX) {
                return Err(BundleError::MalformedComponent(
                    "section size LEB128 exceeds u32",
                ));
            }
            return Ok((value as u32, index + 1));
        }
        shift += 7;
    }
    Err(BundleError::MalformedComponent(
        "section size is not a terminated u32 LEB128",
    ))
}

// ---------------------------------------------------------------------------
// Producer
// ---------------------------------------------------------------------------

/// Builds a component bundle from a component binary: enumerates its
/// CoreModule sections, runs the existing core AoT pipeline
/// ([`telomere::build_aot_artifact`]) per section, and emits the keyed
/// artifact set alongside the component bytes, signed as a whole when
/// `signer` is given.
///
/// The producer parses each embedded core module (it never
/// instantiates anything, so no guest code runs) and the per-section
/// artifacts it emits are byte-identical to standalone core artifacts
/// for the same modules — there is no bundle-specific artifact format.
/// Without a signer the bundle is unsigned (dev mode; loaders refuse
/// it unless explicitly configured otherwise).
pub fn build_component_bundle(
    component: &[u8],
    signer: Option<&dyn AotSigner>,
) -> Result<Vec<u8>, BundleError> {
    let sections = enumerate_core_module_sections(component)?;
    let mut entries: Vec<(u32, [u8; 32], Vec<u8>)> = Vec::with_capacity(sections.len());
    for section in &sections {
        let section_bytes = &component[section.range.clone()];
        let hash: [u8; 32] = Sha256::digest(section_bytes).into();
        let mut reader = IoReadBinaryReader::from(section_bytes);
        let mut parser = WasmParser::new(&mut reader);
        let module = parser
            .parse_module()
            .map_err(|source| BundleError::CoreModuleParse {
                ordinal: section.ordinal,
                source,
            })?;
        // Inner artifacts are unsigned by construction: the bundle
        // signature below is the single trust root for the whole set.
        let artifact = telomere::build_aot_artifact(&module, None).map_err(|source| {
            BundleError::CoreModuleBuild {
                ordinal: section.ordinal,
                source,
            }
        })?;
        entries.push((section.ordinal, hash, artifact));
    }

    let mut out = Vec::new();
    out.extend_from_slice(BUNDLE_MAGIC);
    out.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
    write_blob(&mut out, component);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (ordinal, hash, artifact) in &entries {
        out.extend_from_slice(&ordinal.to_le_bytes());
        out.extend_from_slice(hash);
        write_blob(&mut out, artifact);
    }
    let (key_id, signature) = match signer {
        Some(signer) => (signer.key_id().to_owned(), signer.sign(&out)),
        None => (String::new(), Vec::new()),
    };
    write_key_id(&mut out, &key_id);
    write_blob(&mut out, &signature);
    Ok(out)
}

fn write_blob(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn write_key_id(out: &mut Vec<u8>, key_id: &str) {
    out.extend_from_slice(&(key_id.len() as u32).to_le_bytes());
    out.extend_from_slice(key_id.as_bytes());
}

// ---------------------------------------------------------------------------
// Envelope parsing and signature posture
// ---------------------------------------------------------------------------

/// Bounds-checked little-endian reader over the bundle container.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn offset(&self) -> usize {
        self.pos
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], BundleError> {
        // The length comes from the (untrusted) bundle bytes, so the
        // end offset must be computed with checked arithmetic: an
        // unchecked `pos + len` overflows and panics in builds with
        // overflow checks instead of surfacing a typed error.
        let end = self.pos.checked_add(len).ok_or(BundleError::Truncated {
            offset: self.pos,
            needed: len,
        })?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(BundleError::Truncated {
                offset: self.pos,
                needed: len,
            })?;
        self.pos = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, BundleError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4-byte slice"),
        ))
    }

    fn blob(&mut self) -> Result<&'a [u8], BundleError> {
        let len = u64::from_le_bytes(self.take(8)?.try_into().expect("8-byte slice"));
        let len = usize::try_from(len).map_err(|_| BundleError::LimitExceeded {
            what: "blob length",
            value: len,
        })?;
        self.take(len)
    }

    fn key_id(&mut self) -> Result<String, BundleError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| BundleError::LimitExceeded {
            what: "key id (not valid UTF-8)",
            value: len as u64,
        })
    }
}

/// The parsed outer envelope of a bundle. Field contents are borrowed
/// and uninterpreted until the signature posture has passed.
pub(crate) struct BundleEnvelope<'a> {
    /// The embedded component binary.
    pub component: &'a [u8],
    /// `(ordinal, section hash, artifact bytes)` entries, in file order.
    pub entries: Vec<(u32, [u8; 32], &'a [u8])>,
    /// Signing key id (empty for an unsigned bundle).
    pub key_id: String,
    /// Signature bytes (empty for an unsigned bundle).
    pub signature: &'a [u8],
    /// Number of leading bytes the signature covers (everything before
    /// the signature block).
    pub payload_len: usize,
}

pub(crate) fn parse_bundle_envelope(bytes: &[u8]) -> Result<BundleEnvelope<'_>, BundleError> {
    if !is_component_bundle(bytes) {
        return Err(BundleError::BadMagic);
    }
    let mut reader = Reader::new(&bytes[BUNDLE_MAGIC.len()..]);
    let version = reader.u32()?;
    if version != BUNDLE_FORMAT_VERSION {
        return Err(BundleError::UnsupportedFormatVersion {
            found: version,
            supported: BUNDLE_FORMAT_VERSION,
        });
    }
    let component = reader.blob()?;
    let count = reader.u32()?;
    if u64::from(count) > MAX_CORE_MODULES {
        return Err(BundleError::LimitExceeded {
            what: "artifact entry count",
            value: u64::from(count),
        });
    }
    let mut entries = Vec::with_capacity(count as usize);
    let mut previous_ordinal: Option<u32> = None;
    for _ in 0..count {
        let ordinal = reader.u32()?;
        if previous_ordinal.is_some_and(|prev| ordinal <= prev) {
            return Err(BundleError::LimitExceeded {
                what: "artifact ordinals (not strictly increasing)",
                value: u64::from(ordinal),
            });
        }
        previous_ordinal = Some(ordinal);
        let hash: [u8; 32] = reader.take(32)?.try_into().expect("32-byte slice");
        let artifact = reader.blob()?;
        entries.push((ordinal, hash, artifact));
    }
    let payload_len = BUNDLE_MAGIC.len() + reader.offset();
    let key_id = reader.key_id()?;
    let signature = reader.blob()?;
    if reader.remaining() != 0 {
        return Err(BundleError::TrailingBytes {
            extra: reader.remaining(),
        });
    }
    Ok(BundleEnvelope {
        component,
        entries,
        key_id,
        signature,
        payload_len,
    })
}

/// Applies the bundle-level signature posture, mirroring
/// [`open_aot_artifact`]: a signed bundle requires a verifier with a
/// matching key id and a valid signature; an unsigned bundle requires
/// the explicit [`AotLoadPolicy::allow_unsigned`] opt-in.
///
/// [`open_aot_artifact`]: telomere::open_aot_artifact
pub(crate) fn verify_bundle_signature(
    bytes: &[u8],
    envelope: &BundleEnvelope<'_>,
    verifier: Option<&dyn AotVerifier>,
    policy: AotLoadPolicy,
) -> Result<(), BundleError> {
    if envelope.signature.is_empty() {
        if !policy.allow_unsigned {
            return Err(BundleError::UnsignedRefused);
        }
        return Ok(());
    }
    let verifier = verifier.ok_or_else(|| BundleError::NoVerifier {
        key_id: envelope.key_id.clone(),
    })?;
    if verifier.key_id() != envelope.key_id {
        return Err(BundleError::KeyIdMismatch {
            bundle: envelope.key_id.clone(),
            verifier: verifier.key_id().to_owned(),
        });
    }
    let payload = &bytes[..envelope.payload_len];
    if !verifier.verify(payload, envelope.signature) {
        return Err(BundleError::SignatureInvalid {
            key_id: envelope.key_id.clone(),
        });
    }
    Ok(())
}

/// Pre-checks the keyed artifact set against the embedded component's
/// CoreModule sections and opens every artifact through the #198
/// loader. Runs entirely before component decoding, so a mismatch is
/// reported without any part of the component being interpreted
/// against the wrong artifact set.
pub(crate) fn open_bundle_artifacts(
    envelope: &BundleEnvelope<'_>,
) -> Result<HashMap<u32, AotArtifact>, BundleError> {
    let sections = enumerate_core_module_sections(envelope.component)?;
    let mut entries: HashMap<u32, ([u8; 32], &[u8])> = envelope
        .entries
        .iter()
        .map(|(ordinal, hash, artifact)| (*ordinal, (*hash, *artifact)))
        .collect();
    let mut opened = HashMap::with_capacity(sections.len());
    for section in &sections {
        let (expected, artifact_bytes) =
            entries
                .remove(&section.ordinal)
                .ok_or(BundleError::MissingArtifact {
                    ordinal: section.ordinal,
                })?;
        let section_bytes = &envelope.component[section.range.clone()];
        let actual: [u8; 32] = Sha256::digest(section_bytes).into();
        if actual != expected {
            return Err(BundleError::HashMismatch {
                ordinal: section.ordinal,
                expected: sha256_hex(&expected),
                actual: sha256_hex(&actual),
            });
        }
        // The bundle signature (verified by the caller before this
        // point, or explicitly waived by policy) is the trust root for
        // the whole set, so inner artifacts — unsigned by construction
        // — are opened under the unsigned opt-in here. Fingerprint and
        // module-binding checks inside the #198 loader still apply.
        let artifact = telomere::open_aot_artifact(
            artifact_bytes,
            None,
            AotLoadPolicy {
                allow_unsigned: true,
            },
        )
        .map_err(|source| BundleError::Artifact {
            ordinal: section.ordinal,
            source,
        })?;
        opened.insert(section.ordinal, artifact);
    }
    if let Some(ordinal) = entries.keys().min().copied() {
        return Err(BundleError::ExtraneousArtifact { ordinal });
    }
    Ok(opened)
}

// ---------------------------------------------------------------------------
// Decoder substitution
// ---------------------------------------------------------------------------

/// Decode-session state that substitutes opened bundle artifacts for
/// per-section core module parsing. Installed on the decoder's parse
/// state by the bundle load path only; ordinary compilation never
/// creates one, so the plain decode path is unchanged.
pub(crate) struct CoreModuleSubstitution {
    next_ordinal: u32,
    pending: HashMap<u32, AotArtifact>,
    in_flight: Option<(u32, AotArtifact)>,
    used: Vec<(GlobalIdx<CoreModule>, AotArtifact)>,
    substituted_ordinals: Vec<u32>,
}

impl CoreModuleSubstitution {
    pub(crate) fn new(pending: HashMap<u32, AotArtifact>) -> Self {
        Self {
            next_ordinal: 0,
            pending,
            in_flight: None,
            used: Vec::new(),
            substituted_ordinals: Vec::new(),
        }
    }

    /// Called by the decoder for each CoreModule section, with the
    /// section's bounded reader. Drains the reader (the section bytes
    /// are not parsed) and returns the artifact's deserialized module.
    ///
    /// The bundle load path pre-verifies that every section has an
    /// artifact, so a missing entry means the substitution invariant
    /// was broken internally; the decoder fails closed with
    /// [`ComponentParseError::BundleSubstitutionMissing`] instead of
    /// silently falling back to parsing.
    pub(crate) fn take_for_section(
        &mut self,
        reader: &mut impl BinaryReader,
    ) -> Result<Option<Module>, ComponentParseError> {
        let ordinal = self.next_ordinal;
        self.next_ordinal += 1;
        let artifact = self
            .pending
            .remove(&ordinal)
            .ok_or(ComponentParseError::BundleSubstitutionMissing { ordinal })?;
        drain_reader(reader)?;
        let module = artifact.module().clone();
        self.in_flight = Some((ordinal, artifact));
        Ok(Some(module))
    }

    /// Records the artifact consumed by the most recent
    /// [`Self::take_for_section`] against the global index the decoder
    /// registered the substituted module under.
    pub(crate) fn bind(&mut self, idx: GlobalIdx<CoreModule>) {
        if let Some((ordinal, artifact)) = self.in_flight.take() {
            self.substituted_ordinals.push(ordinal);
            self.used.push((idx, artifact));
        }
    }

    /// Consumes the substitution state after a successful decode,
    /// returning the `(module index, artifact)` pairs in registration
    /// order and the substituted ordinals.
    pub(crate) fn into_outcome(self) -> (Vec<(GlobalIdx<CoreModule>, AotArtifact)>, Vec<u32>) {
        (self.used, self.substituted_ordinals)
    }
}

fn drain_reader(reader: &mut impl BinaryReader) -> Result<(), ComponentParseError> {
    let mut scratch = [0u8; 4096];
    loop {
        match reader.read_slice(&mut scratch)? {
            0 => return Ok(()),
            _ => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// Load result and instantiation artifacts
// ---------------------------------------------------------------------------

/// Phase accounting for a bundle load: which CoreModule sections were
/// substituted from artifacts and which (if any) were parsed. A
/// successful bundle load substitutes every section — the load is
/// fail-closed, so `parsed_ordinals` is empty unless a future load
/// mode deliberately mixes the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleLoadReport {
    /// Total CoreModule sections in the embedded component.
    pub core_module_sections: u32,
    /// Ordinals whose `parse_module` call was skipped in favour of a
    /// bundle artifact, in encounter order.
    pub substituted_ordinals: Vec<u32>,
    /// Ordinals that were parsed from the component bytes.
    pub parsed_ordinals: Vec<u32>,
}

/// A component loaded from a bundle: the decoded program, the opened
/// artifacts keyed by the program's core module indices, the load
/// report, and the bundle's own signature state. Instantiate it with
/// [`ComponentEngine::instantiate_bundle`].
///
/// [`ComponentEngine::instantiate_bundle`]: crate::ComponentEngine::instantiate_bundle
pub struct LoadedComponentBundle {
    program: ComponentProgram,
    artifacts: ComponentBundleArtifacts,
    report: BundleLoadReport,
    key_id: String,
    unsigned: bool,
}

impl LoadedComponentBundle {
    pub(crate) fn new(
        program: ComponentProgram,
        artifacts: ComponentBundleArtifacts,
        report: BundleLoadReport,
        key_id: String,
        unsigned: bool,
    ) -> Self {
        Self {
            program,
            artifacts,
            report,
            key_id,
            unsigned,
        }
    }

    /// The decoded component program (its core modules are the
    /// artifacts' deserialized modules).
    pub fn program(&self) -> &ComponentProgram {
        &self.program
    }

    /// The opened artifacts, for instantiation and install reporting.
    pub fn artifacts(&self) -> &ComponentBundleArtifacts {
        &self.artifacts
    }

    /// The load's phase accounting.
    pub fn report(&self) -> &BundleLoadReport {
        &self.report
    }

    /// The signing key id, or empty for an unsigned bundle.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Whether the bundle carried no signature (accepted only
    /// under an explicit opt-in policy).
    pub fn is_unsigned(&self) -> bool {
        self.unsigned
    }
}

/// The opened AoT artifacts of a loaded bundle, keyed by the core
/// module indices of the decoded program. Shared with the component
/// runtime during bundle instantiation; each defined core instance
/// created from a keyed module gets its artifact installed, and the
/// install reports accumulate here.
#[derive(Clone)]
pub struct ComponentBundleArtifacts {
    set: Rc<BundleArtifactSet>,
}

pub(crate) struct BundleArtifactSet {
    by_module: HashMap<GlobalIdx<CoreModule>, Rc<AotArtifact>>,
    install_reports: RefCell<Vec<(GlobalIdx<CoreModule>, AotInstallReport)>>,
}

impl ComponentBundleArtifacts {
    pub(crate) fn new(used: Vec<(GlobalIdx<CoreModule>, AotArtifact)>) -> Self {
        let by_module = used
            .into_iter()
            .map(|(idx, artifact)| (idx, Rc::new(artifact)))
            .collect();
        Self {
            set: Rc::new(BundleArtifactSet {
                by_module,
                install_reports: RefCell::new(Vec::new()),
            }),
        }
    }

    pub(crate) fn set(&self) -> Rc<BundleArtifactSet> {
        self.set.clone()
    }

    /// Install reports accumulated by bundle instantiations so far,
    /// as `(core module index, report)` pairs in installation order.
    /// A module instantiated more than once (in separate component
    /// instances) contributes one entry per installation.
    pub fn install_reports(&self) -> Vec<(u32, AotInstallReport)> {
        self.set
            .install_reports
            .borrow()
            .iter()
            .map(|(idx, report)| (crate::ir::AnyGlobalIdx::from(*idx).raw() as u32, *report))
            .collect()
    }
}

impl BundleArtifactSet {
    /// Installs the artifact keyed to `module_idx` into `instance`,
    /// if the module has one. Called by the component runtime right
    /// after a defined core instance is created from a substituted
    /// module.
    pub(crate) fn install_for(
        &self,
        store: &Store,
        module_idx: GlobalIdx<CoreModule>,
        instance: &InstanceHandle,
    ) -> Result<(), ComponentError> {
        let Some(artifact) = self.by_module.get(&module_idx) else {
            return Ok(());
        };
        let report = telomere::install_aot_artifact(store, instance, artifact).map_err(|err| {
            ComponentError::Runtime(format!(
                "AoT artifact install for bundled core module failed: {err}"
            ))
        })?;
        self.install_reports.borrow_mut().push((module_idx, report));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leb(mut value: u32) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                return out;
            }
        }
    }

    fn header() -> Vec<u8> {
        b"\0asm\x0d\0\x01\0".to_vec()
    }

    fn section(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        out.extend_from_slice(&leb(payload.len() as u32));
        out.extend_from_slice(payload);
        out
    }

    fn core_module(constant: i32) -> Vec<u8> {
        wat::parse_str(format!(
            "(module (func (export \"f\") (result i32) i32.const {constant}))"
        ))
        .expect("module wat must assemble")
    }

    #[test]
    fn walker_assigns_depth_first_ordinals_across_nesting() {
        let a = core_module(1);
        let b = core_module(2);
        let c = core_module(3);
        let mut inner = header();
        inner.extend_from_slice(&section(SECTION_CORE_MODULE, &b));
        let mut component = header();
        component.extend_from_slice(&section(SECTION_CORE_MODULE, &a));
        component.extend_from_slice(&section(SECTION_COMPONENT, &inner));
        component.extend_from_slice(&section(SECTION_CORE_MODULE, &c));

        let sections = enumerate_core_module_sections(&component).expect("walk");
        assert_eq!(sections.len(), 3);
        let payloads: Vec<&[u8]> = sections
            .iter()
            .map(|section| &component[section.range.clone()])
            .collect();
        assert_eq!(payloads, vec![a.as_slice(), b.as_slice(), c.as_slice()]);
        assert_eq!(
            sections.iter().map(|s| s.ordinal).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn walker_rejects_bad_magic_and_truncation() {
        assert!(matches!(
            enumerate_core_module_sections(b"not a component"),
            Err(BundleError::MalformedComponent(_))
        ));
        // Section size runs past the end of the input.
        let mut component = header();
        component.extend_from_slice(&section(SECTION_CORE_MODULE, &core_module(1)));
        let truncated = &component[..component.len() - 2];
        assert!(matches!(
            enumerate_core_module_sections(truncated),
            Err(BundleError::Truncated { .. })
        ));
        // Unterminated LEB128 section size.
        let mut component = header();
        component.push(SECTION_CORE_MODULE);
        component.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80]);
        assert!(matches!(
            enumerate_core_module_sections(&component),
            Err(BundleError::MalformedComponent(_))
        ));
    }

    #[test]
    fn bundle_magic_detection() {
        assert!(is_component_bundle(b"TELOBND1rest"));
        assert!(!is_component_bundle(b"TELOBND2rest"));
        assert!(!is_component_bundle(b"TELO"));
        assert!(!is_component_bundle(b""));
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn envelope_rejects_overflowing_blob_length() {
        // A crafted bundle whose component blob length is u64::MAX:
        // computing the slice end must not overflow (in builds with
        // overflow checks this panicked before the checked-add fix)
        // and must surface a typed error from envelope parsing,
        // before any signature posture is applied.
        let mut bytes = BUNDLE_MAGIC.to_vec();
        bytes.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            parse_bundle_envelope(&bytes),
            Err(BundleError::Truncated { .. })
        ));
    }
}
