use super::*;

mod branch;
mod compare_select;
mod memory;
mod producer_seed;
mod scalar;

pub use self::{branch::*, compare_select::*, memory::*, producer_seed::*, scalar::*};

#[inline(always)]
fn local_u32(stack: &Stack, local_reference: &LocalReference, local_addr: u32) -> u32 {
    stack.local_read_u32(local_reference, local_addr as usize)
}

#[inline(always)]
fn write_local_u32(
    stack: &mut Stack,
    local_reference: &LocalReference,
    local_addr: u32,
    value: u32,
) {
    stack.local_write_u32(local_reference, local_addr as usize, value);
}

#[inline(always)]
fn local_u64(stack: &Stack, local_reference: &LocalReference, local_addr: u32) -> u64 {
    stack.local_read_u64(local_reference, local_addr as usize)
}

#[inline(always)]
fn write_local_u64(
    stack: &mut Stack,
    local_reference: &LocalReference,
    local_addr: u32,
    value: u64,
) {
    stack.local_write_u64(local_reference, local_addr as usize, value);
}
