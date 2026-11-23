//! Emission environments for the baseline JIT emitters (#197).
//!
//! The baseline emitters used to bake two kinds of process-local values
//! directly into the emitted bytes — stub addresses and the funcaddr of a
//! direct-call callee — and to read call recipes from the live store. That
//! made the output unusable outside the process that produced it. The
//! emitters are now parameterized over an [`EmitEnv`]:
//!
//! - [`JitEmitEnv`] reproduces the historical behavior exactly: recipes
//!   come from the live store and immediates are the real addresses.
//! - [`AotEmitEnv`] has no store. Recipes are derived from module-level
//!   facts (the parsed [`Module`]), stub and funcaddr immediates are
//!   emitted as placeholders, and every placeholder site is recorded as a
//!   [`Reloc`] for the loader to patch.
//!
//! AoT mode takes the wasm direct-call fast path only for functions
//! defined in the same module; an imported callee always goes through the
//! generic helper, because its store identity is unknowable offline.

use crate::common::store::{CallDispatchTarget, CallRecipe, StoreInner};
use crate::common::{FuncType, ImportDesc, Instr, Module, ObjectRef, TypeIdx};
use crate::runtime::jit::stubs::StubId;

/// The kind of a recorded relocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum RelocKind {
    /// The site materializes the address of the given runtime stub.
    Stub(StubId),
    /// The site materializes the funcaddr of the callee with this index
    /// in the module function index space. The loader resolves the index
    /// against the instantiated module, so a future typed-handle rework
    /// (#106) only changes where the loader's value comes from.
    FuncAddr(u32),
}

/// One relocation: a placeholder immediate at `offset` in the emitted
/// bytes, to be patched with the value named by `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reloc {
    pub offset: u32,
    pub kind: RelocKind,
}

/// The result of an AoT-mode emission: code bytes whose relocation sites
/// hold placeholder immediates, plus the relocation table describing
/// them. This is an in-memory type; the artifact format that serializes
/// it belongs to the AoT container work, not to #197.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Emission {
    pub bytes: Vec<u8>,
    pub relocs: Vec<Reloc>,
}

/// A callee the emitter may take the wasm direct-call fast path to.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum FastCallee {
    /// JIT mode: the callee's store identity.
    Store(ObjectRef),
    /// AoT mode: the callee's index in the module function index space.
    Module(u32),
}

/// The call-recipe facts the emitters consume, normalized across modes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EmitRecipe {
    pub param_size: u32,
    pub return_size: u32,
    /// The fast-path callee, if the callee is a wasm function the mode
    /// may call directly. `None` selects the generic call helper.
    pub fast_callee: Option<FastCallee>,
}

/// Everything the baseline emitters need from the world outside the
/// function being emitted. Implementations are monomorphized into the
/// emitters; there is no dynamic dispatch on the emission path.
pub(crate) trait EmitEnv {
    /// Resolves the call recipe for the direct call whose operand sits
    /// at `operand_index` in `wasm`, emitted for `current_funcaddr`.
    fn direct_call_recipe(
        &self,
        wasm: &[Instr],
        operand_index: usize,
        current_funcaddr: ObjectRef,
    ) -> Result<EmitRecipe, ()>;

    /// Resolves the `(param_size, return_size)` layout for the indirect
    /// call whose operand sits at `operand_index` in `wasm`.
    fn indirect_call_layout(
        &self,
        wasm: &[Instr],
        operand_index: usize,
        current_funcaddr: ObjectRef,
    ) -> Result<(u32, u32), ()>;

    /// Returns the immediate to materialize for stub `id` at byte
    /// `offset`. JIT mode returns the stub's address in this process;
    /// AoT mode records a relocation and returns a placeholder.
    fn stub_immediate(&mut self, id: StubId, offset: usize) -> u64;

    /// Returns the immediate to materialize for the fast-path callee at
    /// byte `offset`. JIT mode returns the callee's store identity; AoT
    /// mode records a relocation and returns a placeholder.
    fn funcaddr_immediate(&mut self, callee: FastCallee, offset: usize) -> u32;

    /// Drains the relocations recorded so far. JIT mode records none.
    fn take_relocs(&mut self) -> Vec<Reloc>;
}

fn stack_layout(functype: &FuncType) -> (u32, u32) {
    let param_size = functype.0.iter().map(|ty| ty.stack_size().u32()).sum();
    let return_size = functype.1.iter().map(|ty| ty.stack_size().u32()).sum();
    (param_size, return_size)
}

fn recipe_from_store(recipe: CallRecipe) -> EmitRecipe {
    EmitRecipe {
        param_size: recipe.param_size,
        return_size: recipe.return_size,
        fast_callee: match recipe.target {
            CallDispatchTarget::Wasm { .. } => Some(FastCallee::Store(recipe.frame.code_addr)),
            CallDispatchTarget::Host(_) | CallDispatchTarget::AsyncHost(_) => None,
        },
    }
}

/// The JIT emission environment: recipes and immediates come from the
/// live store, exactly as the emitters behaved before #197.
pub(crate) struct JitEmitEnv<'a> {
    runtime: &'a StoreInner,
}

impl<'a> JitEmitEnv<'a> {
    pub(crate) fn new(runtime: &'a StoreInner) -> Self {
        Self { runtime }
    }
}

impl EmitEnv for JitEmitEnv<'_> {
    fn direct_call_recipe(
        &self,
        wasm: &[Instr],
        operand_index: usize,
        current_funcaddr: ObjectRef,
    ) -> Result<EmitRecipe, ()> {
        let recipe_ref = unsafe { wasm.get(operand_index).ok_or(())?.operand.call_recipe_ref };
        if let Some(recipe_slot) = recipe_ref.resolved_recipe_slot() {
            if let Some(recipe) = self.runtime.call_recipe(recipe_slot) {
                return Ok(recipe_from_store(recipe));
            }
        }
        let current_func = self.runtime.get_func(current_funcaddr);
        let callee = self
            .runtime
            .instance(current_func.instance)
            .funcs
            .as_slice()
            .get(recipe_ref.funcidx as usize)
            .copied()
            .ok_or(())?;
        Ok(recipe_from_store(self.runtime.build_call_recipe(callee)))
    }

    fn indirect_call_layout(
        &self,
        wasm: &[Instr],
        operand_index: usize,
        current_funcaddr: ObjectRef,
    ) -> Result<(u32, u32), ()> {
        let expected_typeidx =
            unsafe { wasm.get(operand_index + 1).ok_or(())?.operand.u32 as usize };
        let current_func = self.runtime.get_func(current_funcaddr);
        let instance = self.runtime.instance(current_func.instance);
        let module = self.runtime.get_module(instance.module_addr);
        let functype = module.function_types.get(expected_typeidx).ok_or(())?;
        Ok(stack_layout(functype))
    }

    fn stub_immediate(&mut self, id: StubId, _offset: usize) -> u64 {
        id.address() as u64
    }

    fn funcaddr_immediate(&mut self, callee: FastCallee, _offset: usize) -> u32 {
        match callee {
            FastCallee::Store(funcaddr) => funcaddr.get(),
            FastCallee::Module(_) => {
                unreachable!("the JIT environment only produces store callees")
            }
        }
    }

    fn take_relocs(&mut self) -> Vec<Reloc> {
        Vec::new()
    }
}

/// The module-level facts an [`AotEmitEnv`] reads: the parsed module's
/// function index space and type section.
#[allow(dead_code)]
pub(crate) struct ModuleFacts<'a> {
    module: &'a Module,
    imported_funcs: u32,
}

#[allow(dead_code)]
impl<'a> ModuleFacts<'a> {
    pub(crate) fn new(module: &'a Module) -> Self {
        let imported_funcs = module
            .imports
            .0
            .iter()
            .filter(|import| matches!(import.desc, ImportDesc::TypeIdx(_)))
            .count() as u32;
        Self {
            module,
            imported_funcs,
        }
    }

    /// The number of imported functions; function indices below this are
    /// imports, the rest are defined in this module.
    pub(crate) fn imported_func_count(&self) -> u32 {
        self.imported_funcs
    }

    fn func_type(&self, funcidx: u32) -> Option<&'a FuncType> {
        let typeidx = self.module.functions.get(funcidx as usize)?;
        self.module.fts.get(*typeidx)
    }

    fn type_by_index(&self, typeidx: u32) -> Option<&'a FuncType> {
        self.module.fts.get(TypeIdx(typeidx))
    }
}

/// The AoT emission environment: recipes come from module-level facts
/// and every process-local immediate becomes a recorded relocation with
/// a placeholder in the byte stream.
#[allow(dead_code)]
pub(crate) struct AotEmitEnv<'a> {
    facts: ModuleFacts<'a>,
    relocs: Vec<Reloc>,
}

#[allow(dead_code)]
impl<'a> AotEmitEnv<'a> {
    pub(crate) fn new(facts: ModuleFacts<'a>) -> Self {
        Self {
            facts,
            relocs: Vec::new(),
        }
    }
}

impl EmitEnv for AotEmitEnv<'_> {
    fn direct_call_recipe(
        &self,
        wasm: &[Instr],
        operand_index: usize,
        _current_funcaddr: ObjectRef,
    ) -> Result<EmitRecipe, ()> {
        let recipe_ref = unsafe { wasm.get(operand_index).ok_or(())?.operand.call_recipe_ref };
        let funcidx = recipe_ref.funcidx;
        let functype = self.facts.func_type(funcidx).ok_or(())?;
        let (param_size, return_size) = stack_layout(functype);
        // The fast path needs the callee's store identity, which only
        // exists for functions defined in this same module. Imported
        // callees always take the generic helper path; their sizes still
        // come from the declared type, which instantiation guarantees
        // matches the definition.
        let fast_callee =
            (funcidx >= self.facts.imported_func_count()).then_some(FastCallee::Module(funcidx));
        Ok(EmitRecipe {
            param_size,
            return_size,
            fast_callee,
        })
    }

    fn indirect_call_layout(
        &self,
        wasm: &[Instr],
        operand_index: usize,
        _current_funcaddr: ObjectRef,
    ) -> Result<(u32, u32), ()> {
        let expected_typeidx = unsafe { wasm.get(operand_index + 1).ok_or(())?.operand.u32 };
        let functype = self.facts.type_by_index(expected_typeidx).ok_or(())?;
        Ok(stack_layout(functype))
    }

    fn stub_immediate(&mut self, id: StubId, offset: usize) -> u64 {
        self.relocs.push(Reloc {
            offset: offset as u32,
            kind: RelocKind::Stub(id),
        });
        0
    }

    fn funcaddr_immediate(&mut self, callee: FastCallee, offset: usize) -> u32 {
        match callee {
            FastCallee::Module(funcidx) => {
                self.relocs.push(Reloc {
                    offset: offset as u32,
                    kind: RelocKind::FuncAddr(funcidx),
                });
                0
            }
            FastCallee::Store(_) => {
                unreachable!("the AoT environment only produces module callees")
            }
        }
    }

    fn take_relocs(&mut self) -> Vec<Reloc> {
        std::mem::take(&mut self.relocs)
    }
}
