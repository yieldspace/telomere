//! Compact runtime retention for producer-supplied WebAssembly names.

use super::custom_section::{FuncNameSubSec, ModuleNameSubSec, NameSubSection};
use std::fmt;
#[cfg(test)]
use std::{mem, sync::Arc};

/// Retained module and function names used by later diagnostic consumers.
///
/// Function-name entries are sorted by their WebAssembly function index and
/// refer to byte offsets in one concatenated string blob. Local names are
/// deliberately not retained.
pub(crate) struct ModuleNames {
    module_name: Option<Box<str>>,
    function_entries: Box<[(u32, u32)]>,
    function_name_blob: Box<str>,
}

impl ModuleNames {
    /// Compacts the retained parts of a parsed `name` custom section.
    ///
    /// A module with only local names has no retained data and returns `None`.
    /// The function index space is intentionally not validated against the
    /// module: callers use the core index space directly. Sorting is stable, so
    /// duplicate indices retain the first producer-supplied name.
    pub(crate) fn from_name_section(name_section: NameSubSection) -> Option<Self> {
        let NameSubSection {
            module_name,
            function_name,
            ..
        } = name_section;
        let module_name = module_name.map(|ModuleNameSubSec(name)| name.into_boxed_str());
        let mut function_names = function_name
            .map(|FuncNameSubSec(function_names)| function_names)
            .unwrap_or_default();
        function_names.sort_by_key(|(funcidx, _)| *funcidx);
        function_names.dedup_by_key(|(funcidx, _)| *funcidx);

        if module_name.is_none() && function_names.is_empty() {
            return None;
        }

        let blob_len = function_names.iter().fold(0_usize, |len, (_, name)| {
            len.checked_add(name.len())
                .expect("function-name blob length exceeds usize::MAX")
        });
        let mut function_name_blob = String::with_capacity(blob_len);
        let mut function_entries = Vec::with_capacity(function_names.len());
        for (funcidx, name) in function_names {
            let offset = u32::try_from(function_name_blob.len())
                .expect("function-name blob length exceeds u32::MAX");
            function_entries.push((funcidx, offset));
            function_name_blob.push_str(&name);
        }

        Some(Self {
            module_name,
            function_entries: function_entries.into_boxed_slice(),
            function_name_blob: function_name_blob.into_boxed_str(),
        })
    }

    /// Returns the producer-supplied module name, when one was retained.
    #[allow(dead_code)] // Cold diagnostic seam for #207/#210; not reached on a dispatch path yet.
    pub(crate) fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    /// Returns the retained name for a core WebAssembly function index.
    ///
    /// The lookup accepts every `u32`; an out-of-module index simply has no
    /// entry rather than being range-validated against a module declaration.
    #[allow(dead_code)] // Cold diagnostic seam for #207/#210; not reached on a dispatch path yet.
    pub(crate) fn function_name(&self, funcidx: u32) -> Option<&str> {
        let entry = self
            .function_entries
            .binary_search_by_key(&funcidx, |(entry_funcidx, _)| *entry_funcidx)
            .ok()?;
        let start = self.function_entries[entry].1 as usize;
        let end = self
            .function_entries
            .get(entry + 1)
            .map_or(self.function_name_blob.len(), |(_, offset)| {
                *offset as usize
            });
        self.function_name_blob.get(start..end)
    }

    /// Counts only heap payload owned by this structure, not its allocation
    /// headers, the `Arc` header, or allocator bucket rounding.
    ///
    /// This is test-only logical accounting, not an embedder-facing API.
    #[cfg(test)]
    pub(crate) fn retained_payload_bytes(&self) -> usize {
        self.function_name_blob.len()
            + self.function_entries.len() * mem::size_of::<(u32, u32)>()
            + self.module_name.as_deref().map_or(0, str::len)
    }

    /// Counts logical retained bytes for the measurement probe.
    ///
    /// This is test-only accounting rather than an embedder API. It includes
    /// the payload, `ModuleNames`, the `Option<Arc<ModuleNames>>` slot on
    /// `ModuleInstance`, and an assumed two-`usize` `Arc` control block. The
    /// control-block layout is not stable API and allocator bucket rounding is
    /// intentionally excluded.
    #[cfg(test)]
    pub(crate) fn retained_total_bytes(&self) -> usize {
        self.retained_payload_bytes()
            + mem::size_of::<Self>()
            + Self::arc_control_block_logical_bytes()
            + Self::module_instance_names_slot_bytes()
    }

    /// Returns the number of live allocations for the measurement probe.
    #[cfg(test)]
    pub(crate) fn retained_allocation_count(&self) -> usize {
        1 + usize::from(
            self.module_name
                .as_deref()
                .is_some_and(|module_name| !module_name.is_empty()),
        ) + self.function_name_allocation_count()
    }

    /// Returns allocations for the compact function-name representation.
    #[cfg(test)]
    pub(crate) fn function_name_allocation_count(&self) -> usize {
        usize::from(!self.function_entries.is_empty())
            + usize::from(!self.function_name_blob.is_empty())
    }

    /// Logical size assigned to the implementation-private `Arc` header.
    #[cfg(test)]
    pub(crate) const fn arc_control_block_logical_bytes() -> usize {
        // `Arc`'s control-block layout is not observable through stable Rust.
        // The measurement boundary assumes its two counters occupy two `usize`
        // values on the 64-bit platforms reported by the probe.
        2 * mem::size_of::<usize>()
    }

    /// Size of the `ModuleInstance` field that holds a retained name set.
    #[cfg(test)]
    pub(crate) const fn module_instance_names_slot_bytes() -> usize {
        mem::size_of::<Option<Arc<Self>>>()
    }
}

impl fmt::Debug for ModuleNames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModuleNames")
            .field("function_count", &self.function_entries.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::custom_section::{FuncNameSubSec, LocalNameSubSec, ModuleNameSubSec},
        IoReadBinaryReader, WasmParser,
    };
    use std::{
        collections::BTreeSet,
        env, fs,
        path::{Path, PathBuf},
    };

    /// Counts the parser's actual `Vec` and `String` capacities, rather than
    /// normalizing them to lengths. This makes the measurement comparison an
    /// as-is move of the parsed module/function names, not a theoretical
    /// lower bound.
    fn vec_name_section_as_is_logical_bytes(name_section: &NameSubSection) -> usize {
        let module_name_bytes = name_section
            .module_name
            .as_ref()
            .map_or(0, |module_name| module_name.0.capacity());
        let function_names = name_section
            .function_name
            .as_ref()
            .map(|function_names| &function_names.0);
        let function_table_bytes = function_names.map_or(0, |function_names| {
            function_names.capacity() * mem::size_of::<(u32, String)>()
                + function_names
                    .iter()
                    .map(|(_, name)| name.capacity())
                    .sum::<usize>()
        });
        module_name_bytes + function_table_bytes
    }

    fn vec_name_section_as_is_allocation_count(name_section: &NameSubSection) -> usize {
        let module_name_allocations = usize::from(
            name_section
                .module_name
                .as_ref()
                .is_some_and(|module_name| module_name.0.capacity() != 0),
        );
        let function_names = name_section
            .function_name
            .as_ref()
            .map(|function_names| &function_names.0);
        module_name_allocations
            + function_names.map_or(0, |function_names| {
                usize::from(function_names.capacity() != 0)
                    + function_names
                        .iter()
                        .filter(|(_, name)| name.capacity() != 0)
                        .count()
            })
    }

    struct CustomSectionMetrics {
        name_section_payload_bytes: usize,
        dwarf_section_bytes: usize,
    }

    fn custom_section_metrics(bytes: &[u8]) -> Result<CustomSectionMetrics, String> {
        if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
            return Err("missing WebAssembly header".to_owned());
        }

        let mut cursor = 8;
        let mut metrics = CustomSectionMetrics {
            name_section_payload_bytes: 0,
            dwarf_section_bytes: 0,
        };
        while cursor < bytes.len() {
            // An encoded section starts before its id and ends after its
            // payload, so this boundary includes the id and outer size LEB.
            let section_start = cursor;
            let section_id = *bytes
                .get(cursor)
                .ok_or_else(|| "missing section id".to_owned())?;
            cursor += 1;
            let section_size = read_u32_leb(bytes, &mut cursor)? as usize;
            let payload_start = cursor;
            let payload_end = payload_start
                .checked_add(section_size)
                .ok_or_else(|| "section payload length overflow".to_owned())?;
            let payload = bytes
                .get(payload_start..payload_end)
                .ok_or_else(|| "truncated section payload".to_owned())?;

            if section_id == 0 {
                let custom_name = custom_section_name(payload)?;
                if custom_name == "name" {
                    metrics.name_section_payload_bytes = metrics
                        .name_section_payload_bytes
                        .checked_add(section_size)
                        .ok_or_else(|| "name section payload total overflow".to_owned())?;
                }
                if custom_name.starts_with(".debug_") {
                    // DWARF accounting deliberately includes the section id,
                    // outer size LEB, and payload: [section_start, payload_end).
                    let encoded_section_bytes = payload_end
                        .checked_sub(section_start)
                        .ok_or_else(|| "encoded section length underflow".to_owned())?;
                    metrics.dwarf_section_bytes = metrics
                        .dwarf_section_bytes
                        .checked_add(encoded_section_bytes)
                        .ok_or_else(|| "DWARF section total overflow".to_owned())?;
                }
            }
            cursor = payload_end;
        }
        Ok(metrics)
    }

    fn custom_section_name(payload: &[u8]) -> Result<&str, String> {
        let mut name_cursor = 0;
        let custom_name_length = read_u32_leb(payload, &mut name_cursor)? as usize;
        let custom_name_end = name_cursor
            .checked_add(custom_name_length)
            .ok_or_else(|| "custom section name length overflow".to_owned())?;
        let custom_name_bytes = payload
            .get(name_cursor..custom_name_end)
            .ok_or_else(|| "truncated custom section name".to_owned())?;
        std::str::from_utf8(custom_name_bytes)
            .map_err(|_| "custom section name is not valid UTF-8".to_owned())
    }

    fn name_section_payload_bytes(bytes: &[u8]) -> Result<usize, String> {
        Ok(custom_section_metrics(bytes)?.name_section_payload_bytes)
    }

    fn dwarf_section_bytes(bytes: &[u8]) -> Result<usize, String> {
        Ok(custom_section_metrics(bytes)?.dwarf_section_bytes)
    }

    fn read_u32_leb(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
        let mut value = 0_u32;
        for shift in (0..35).step_by(7) {
            let byte = *bytes
                .get(*cursor)
                .ok_or_else(|| "truncated unsigned LEB128".to_owned())?;
            *cursor += 1;
            if shift == 28 && byte & 0xf0 != 0 {
                return Err("unsigned LEB128 exceeds u32".to_owned());
            }
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("unsigned LEB128 exceeds five bytes".to_owned())
    }

    struct ProbeMetrics {
        module_bytes: usize,
        dwarf_section_bytes: usize,
        module_bytes_excluding_dwarf: usize,
        name_section_payload_bytes: usize,
        compact_retained_payload_bytes: usize,
        compact_retained_total_logical_bytes: usize,
        compact_live_allocations: usize,
        vec_as_is_logical_bytes: usize,
        vec_live_allocations: usize,
    }

    fn probe_metrics(path: &Path) -> Result<ProbeMetrics, String> {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let module_bytes = bytes.len();
        let section_metrics = custom_section_metrics(&bytes)?;
        let name_payload_bytes = section_metrics.name_section_payload_bytes;
        let dwarf_bytes = section_metrics.dwarf_section_bytes;
        let module_bytes_excluding_dwarf = module_bytes
            .checked_sub(dwarf_bytes)
            .ok_or_else(|| "DWARF section bytes exceed module bytes".to_owned())?;
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let module = WasmParser::new(&mut reader)
            .parse_module()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let Some(name_section) = module.name else {
            return Ok(ProbeMetrics {
                module_bytes,
                dwarf_section_bytes: dwarf_bytes,
                module_bytes_excluding_dwarf,
                name_section_payload_bytes: name_payload_bytes,
                compact_retained_payload_bytes: 0,
                compact_retained_total_logical_bytes: 0,
                compact_live_allocations: 0,
                vec_as_is_logical_bytes: 0,
                vec_live_allocations: 0,
            });
        };

        let mut funcidxs = BTreeSet::new();
        if let Some(function_names) = &name_section.function_name {
            for (funcidx, _) in &function_names.0 {
                if !funcidxs.insert(*funcidx) {
                    return Err(format!(
                        "{}: duplicate function index {funcidx} cannot be compared as-is",
                        path.display()
                    ));
                }
            }
        }
        let vec_logical_bytes = vec_name_section_as_is_logical_bytes(&name_section);
        let vec_allocations = vec_name_section_as_is_allocation_count(&name_section);
        let Some(compact) = ModuleNames::from_name_section(name_section) else {
            return Ok(ProbeMetrics {
                module_bytes,
                dwarf_section_bytes: dwarf_bytes,
                module_bytes_excluding_dwarf,
                name_section_payload_bytes: name_payload_bytes,
                compact_retained_payload_bytes: 0,
                compact_retained_total_logical_bytes: 0,
                compact_live_allocations: 0,
                vec_as_is_logical_bytes: vec_logical_bytes,
                vec_live_allocations: vec_allocations,
            });
        };
        Ok(ProbeMetrics {
            module_bytes,
            dwarf_section_bytes: dwarf_bytes,
            module_bytes_excluding_dwarf,
            name_section_payload_bytes: name_payload_bytes,
            compact_retained_payload_bytes: compact.retained_payload_bytes(),
            compact_retained_total_logical_bytes: compact.retained_total_bytes(),
            compact_live_allocations: compact.retained_allocation_count(),
            vec_as_is_logical_bytes: vec_logical_bytes,
            vec_live_allocations: vec_allocations,
        })
    }

    fn probe_manifest(path: &Path) -> Result<Vec<(String, PathBuf)>, String> {
        let manifest =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        parse_tsv_probe_manifest(&manifest)
    }

    fn parse_tsv_probe_manifest(manifest: &str) -> Result<Vec<(String, PathBuf)>, String> {
        let mut rows = Vec::new();
        for (line_number, line) in manifest.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let (label, path) = line.split_once('\t').ok_or_else(|| {
                format!("manifest row {} must be label, tab, path", line_number + 1)
            })?;
            validate_probe_label(label)?;
            if path.is_empty() {
                return Err(format!(
                    "manifest row {} has an empty path",
                    line_number + 1
                ));
            }
            rows.push((label.to_owned(), PathBuf::from(path)));
        }
        if rows.is_empty() {
            return Err("probe manifest has no rows".to_owned());
        }
        Ok(rows)
    }

    fn validate_probe_label(label: &str) -> Result<(), String> {
        if label.is_empty()
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(format!(
                "probe label {label:?} must use ASCII letters, digits, '-', '_' or '.'"
            ));
        }
        Ok(())
    }

    fn append_u32_leb(bytes: &mut Vec<u8>, mut value: usize) {
        loop {
            let mut byte = u8::try_from(value & 0x7f).expect("seven bits fit in u8");
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn append_custom_section(
        bytes: &mut Vec<u8>,
        name: &[u8],
        contents: &[u8],
    ) -> (usize, usize, usize) {
        let mut payload = Vec::new();
        append_u32_leb(&mut payload, name.len());
        payload.extend_from_slice(name);
        payload.extend_from_slice(contents);

        let section_start = bytes.len();
        bytes.push(0);
        append_u32_leb(bytes, payload.len());
        let payload_bytes = payload.len();
        bytes.extend_from_slice(&payload);
        (section_start, payload_bytes, bytes.len())
    }

    const MEASUREMENT_PROBE_STACK_BYTES: usize = 64 * 1024 * 1024;

    #[test]
    fn probe_tsv_manifest_accepts_measurement_tool_shape() {
        let rows = parse_tsv_probe_manifest(
            "fixture-add\t/private/tmp/add.wasm\nsynthetic-f10\t/private/tmp/synthetic-f10.wasm\n",
        )
        .expect("the measurement tool's TSV shape must parse");
        assert_eq!(
            rows,
            vec![
                (
                    "fixture-add".to_owned(),
                    PathBuf::from("/private/tmp/add.wasm")
                ),
                (
                    "synthetic-f10".to_owned(),
                    PathBuf::from("/private/tmp/synthetic-f10.wasm"),
                ),
            ]
        );
        assert!(parse_tsv_probe_manifest("fixture-add").is_err());
    }

    #[test]
    fn dwarf_section_bytes_include_encoded_boundaries_and_reject_invalid_names() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        let (_, name_payload_bytes, _) = append_custom_section(&mut wasm, b"name", b"name-payload");
        let (debug_info_start, _, debug_info_end) =
            append_custom_section(&mut wasm, b".debug_info", b"info");
        let (debug_line_start, debug_line_payload_bytes, debug_line_end) =
            append_custom_section(&mut wasm, b".debug_line", &[0; 200]);
        append_custom_section(&mut wasm, b"producers", b"not-dwarf");

        let expected_dwarf_bytes =
            (debug_info_end - debug_info_start) + (debug_line_end - debug_line_start);
        assert!(debug_line_payload_bytes > 127);
        assert_eq!(
            debug_line_end - debug_line_start,
            1 + 2 + debug_line_payload_bytes,
            "the full encoded section includes id and a two-byte outer size LEB"
        );
        assert_eq!(
            name_section_payload_bytes(&wasm).expect("valid custom section names"),
            name_payload_bytes,
            "name-section accounting remains payload-only"
        );
        assert_eq!(
            dwarf_section_bytes(&wasm).expect("valid custom section names"),
            expected_dwarf_bytes,
            "DWARF accounting includes each full encoded custom section"
        );

        let mut invalid_name = b"\0asm\x01\0\0\0".to_vec();
        append_custom_section(&mut invalid_name, &[0xff], b"");
        assert!(custom_section_metrics(&invalid_name).is_err());
        assert!(dwarf_section_bytes(&invalid_name).is_err());
    }

    #[test]
    #[ignore = "invoked by tools/measure-debug-name-retention.py"]
    fn measurement_probe() {
        std::thread::Builder::new()
            .stack_size(MEASUREMENT_PROBE_STACK_BYTES)
            .spawn(run_measurement_probe)
            .expect("measurement probe worker thread must start")
            .join()
            .unwrap_or_else(|_| panic!("measurement probe worker thread panicked"));
    }

    fn run_measurement_probe() {
        assert_eq!(
            usize::BITS,
            64,
            "the logical Arc-header accounting assumption is defined only for 64-bit probes"
        );
        let manifest = env::var_os("TELOMERE_DEBUG_NAMES_PROBE_MANIFEST")
            .expect("TELOMERE_DEBUG_NAMES_PROBE_MANIFEST must point to a TSV manifest");
        let rows = probe_manifest(Path::new(&manifest)).unwrap_or_else(|error| panic!("{error}"));
        for (label, path) in rows {
            let metrics = probe_metrics(&path)
                .unwrap_or_else(|error| panic!("{label} ({}): {error}", path.display()));
            println!(
                "DEBUG_NAME_RETENTION_JSON {{\"label\":\"{label}\",\"module_bytes\":{module_bytes},\"dwarf_section_bytes\":{dwarf_section_bytes},\"module_bytes_excluding_dwarf\":{module_bytes_excluding_dwarf},\"name_section_payload_bytes\":{name_section_payload_bytes},\"compact_retained_payload_bytes\":{compact_retained_payload_bytes},\"compact_retained_total_logical_bytes\":{compact_retained_total_logical_bytes},\"compact_live_allocations\":{compact_live_allocations},\"vec_as_is_logical_bytes\":{vec_as_is_logical_bytes},\"vec_live_allocations\":{vec_live_allocations},\"pointer_width_bits\":{pointer_width_bits},\"module_names_size_bytes\":{module_names_size_bytes},\"option_arc_slot_bytes\":{option_arc_slot_bytes},\"arc_header_assumption_bytes\":{arc_header_assumption_bytes}}}",
                module_bytes = metrics.module_bytes,
                dwarf_section_bytes = metrics.dwarf_section_bytes,
                module_bytes_excluding_dwarf = metrics.module_bytes_excluding_dwarf,
                name_section_payload_bytes = metrics.name_section_payload_bytes,
                compact_retained_payload_bytes = metrics.compact_retained_payload_bytes,
                compact_retained_total_logical_bytes = metrics.compact_retained_total_logical_bytes,
                compact_live_allocations = metrics.compact_live_allocations,
                vec_as_is_logical_bytes = metrics.vec_as_is_logical_bytes,
                vec_live_allocations = metrics.vec_live_allocations,
                pointer_width_bits = usize::BITS,
                module_names_size_bytes = mem::size_of::<ModuleNames>(),
                option_arc_slot_bytes = ModuleNames::module_instance_names_slot_bytes(),
                arc_header_assumption_bytes = ModuleNames::arc_control_block_logical_bytes(),
            );
        }
    }

    fn name_section(module_name: Option<&str>, function_names: Vec<(u32, &str)>) -> NameSubSection {
        NameSubSection {
            module_name: module_name.map(|name| ModuleNameSubSec(name.to_owned())),
            function_name: Some(FuncNameSubSec(
                function_names
                    .into_iter()
                    .map(|(funcidx, name)| (funcidx, name.to_owned()))
                    .collect(),
            )),
            local_name: None,
        }
    }

    #[test]
    fn function_names_are_sorted_and_first_duplicate_wins() {
        let names = ModuleNames::from_name_section(name_section(
            Some("module-name"),
            vec![
                (4, "four"),
                (1, "first"),
                (u32::MAX, "maximum"),
                (4, "later-duplicate"),
            ],
        ))
        .expect("module and function names must be retained");

        assert_eq!(names.module_name(), Some("module-name"));
        assert_eq!(
            names.function_entries.as_ref(),
            &[(1, 0), (4, 5), (u32::MAX, 9)]
        );
        assert_eq!(names.function_name(1), Some("first"));
        assert_eq!(names.function_name(4), Some("four"));
        assert_eq!(names.function_name(u32::MAX), Some("maximum"));
        assert_eq!(names.function_name(2), None);
    }

    #[test]
    fn empty_or_local_only_name_sections_do_not_panic() {
        assert!(ModuleNames::from_name_section(name_section(None, vec![])).is_none());
        assert!(ModuleNames::from_name_section(NameSubSection {
            module_name: None,
            function_name: None,
            local_name: Some(LocalNameSubSec(vec![(0, vec![(0, "local".to_owned())])])),
        })
        .is_none());

        let empty_module = ModuleNames::from_name_section(name_section(Some(""), vec![]))
            .expect("an explicitly empty module name is retained");
        assert_eq!(empty_module.module_name(), Some(""));

        let empty_function = ModuleNames::from_name_section(name_section(None, vec![(0, "")]))
            .expect("an explicitly empty function name is retained");
        assert_eq!(empty_function.function_name(0), Some(""));
    }

    #[test]
    fn debug_hides_producer_symbols() {
        let names = ModuleNames::from_name_section(name_section(
            Some("module-secret"),
            vec![(7, "function-secret")],
        ))
        .expect("names must be retained");

        let debug = format!("{names:?}");
        assert!(debug.contains("function_count"));
        assert!(!debug.contains("module-secret"));
        assert!(!debug.contains("function-secret"));
    }

    #[test]
    fn logical_accounting_keeps_payload_and_instance_costs_distinct() {
        let names =
            ModuleNames::from_name_section(name_section(Some("m"), vec![(1, "abc"), (2, "d")]))
                .expect("names must be retained");
        let as_is = name_section(Some("m"), vec![(1, "abc"), (2, "d")]);

        assert_eq!(
            names.retained_payload_bytes(),
            1 + 2 * mem::size_of::<(u32, u32)>() + 4
        );
        assert_eq!(
            names.retained_total_bytes(),
            names.retained_payload_bytes()
                + mem::size_of::<ModuleNames>()
                + 2 * mem::size_of::<usize>()
                + mem::size_of::<Option<Arc<ModuleNames>>>()
        );
        assert_eq!(names.function_name_allocation_count(), 2);
        assert_eq!(names.retained_allocation_count(), 4);
        assert_eq!(
            vec_name_section_as_is_logical_bytes(&as_is),
            1 + 2 * mem::size_of::<(u32, String)>() + 4
        );
        assert_eq!(vec_name_section_as_is_allocation_count(&as_is), 4);
    }
}
