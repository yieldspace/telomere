use std::slice::SliceIndex;

use super::{VMResult, PAGE_SIZE};
#[derive(Debug, Clone, Copy)]
pub struct MemArg {
    pub align: u32,
    pub offset: u32,
}
pub struct Memory(Vec<u8>, u32);
fn compute_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(
        memarg.offset.checked_add(offset).map(|v| v as usize),
        || VMResult::MemoryIndexOutOfRange,
    )
}
impl Memory {
    pub fn new(page_count: u32, max_page_size: u32) -> Self {
        Self(vec![0; page_count as usize * PAGE_SIZE], max_page_size)
    }
    pub fn get_mut<I: SliceIndex<[u8]>>(&mut self, index: I) -> Option<&mut I::Output> {
        self.0.get_mut(index)
    }
    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> VMResult<[u8; N]> {
        let mut arr = [0u8; N];
        let last = vm_try!(VMResult::from_option(offset.checked_add(N), || {
            VMResult::StackOverflow
        }));
        arr.copy_from_slice(vm_try!(VMResult::from_option(
            self.0.get(offset..last),
            || { VMResult::MemoryIndexOutOfRange }
        )));
        VMResult::Success(arr)
    }
    pub fn init(&mut self, offset: u32, value: &[u8]) -> VMResult<()> {
        let offset = offset as usize;
        let last = vm_try!(VMResult::from_option(
            offset.checked_add(value.len()),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        vm_try!(VMResult::from_option(self.0.get_mut(offset..last), || {
            VMResult::MemoryIndexOutOfRange
        }))
        .copy_from_slice(value);
        VMResult::Success(())
    }
    fn write_slice(&mut self, memarg: MemArg, offset: u32, value: &[u8]) -> VMResult<()> {
        let offset = vm_try!(compute_offset(memarg, offset));
        let n = value.len();
        let last = vm_try!(VMResult::from_option(offset.checked_add(n), || {
            VMResult::MemoryIndexOutOfRange
        }));
        vm_try!(VMResult::from_option(self.0.get_mut(offset..last), || {
            VMResult::MemoryIndexOutOfRange
        }))
        .copy_from_slice(value);
        VMResult::Success(())
    }
    pub fn write_f32(&mut self, memarg: MemArg, offset: u32, value: f32) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_f64(&mut self, memarg: MemArg, offset: u32, value: f64) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u32(&mut self, memarg: MemArg, offset: u32, value: u32) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u64(&mut self, memarg: MemArg, offset: u32, value: u64) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u8(&mut self, memarg: MemArg, offset: u32, value: u8) -> VMResult<()> {
        *vm_try!(VMResult::from_option(
            self.0.get_mut(vm_try!(compute_offset(memarg, offset))),
            || VMResult::MemoryIndexOutOfRange
        )) = value;

        VMResult::Success(())
    }
    pub fn write_u16(&mut self, memarg: MemArg, offset: u32, value: u16) -> VMResult<()> {
        vm_try!(self.write_slice(memarg, offset, &value.to_le_bytes()));
        VMResult::Success(())
    }
    pub fn read_i32(&self, memarg: MemArg, offset: u32) -> VMResult<i32> {
        VMResult::Success(i32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u32(&self, memarg: MemArg, offset: u32) -> VMResult<u32> {
        VMResult::Success(u32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u64(&self, memarg: MemArg, offset: u32) -> VMResult<u64> {
        VMResult::Success(u64::from_le_bytes(vm_try!(
            self.read_u8_array::<8>(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_f32(&self, memarg: MemArg, offset: u32) -> VMResult<f32> {
        VMResult::Success(f32::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_f64(&self, memarg: MemArg, offset: u32) -> VMResult<f64> {
        VMResult::Success(f64::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u8(&self, memarg: MemArg, offset: u32) -> VMResult<u8> {
        VMResult::Success(
            vm_try!(self.read_u8_array::<1>(vm_try!(compute_offset(memarg, offset))))[0],
        )
    }
    pub fn read_i8(&self, memarg: MemArg, offset: u32) -> VMResult<i8> {
        VMResult::Success(
            vm_try!(self.read_u8_array::<1>(vm_try!(compute_offset(memarg, offset))))[0] as i8,
        )
    }
    pub fn read_i16(&self, memarg: MemArg, offset: u32) -> VMResult<i16> {
        VMResult::Success(i16::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u16(&self, memarg: MemArg, offset: u32) -> VMResult<u16> {
        VMResult::Success(u16::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn page_size(&self) -> u32 {
        (self.0.len() / PAGE_SIZE) as u32
    }
    pub fn grow(&mut self, page_size_delta: u32) -> VMResult<i32> {
        let current_page_size = self.page_size();
        let new_page_size = current_page_size + page_size_delta;

        if self.1 >= new_page_size {
            // FIXME: check memory allocation and new length
            self.0.resize((new_page_size) as usize * PAGE_SIZE, 0);
            VMResult::Success(current_page_size as i32)
        } else {
            VMResult::Success(-1)
        }
    }
    pub fn fill(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        let last = vm_try!(VMResult::from_option(ptr.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        }));
        let slice = vm_try!(VMResult::from_option(
            self.0.get_mut(ptr as usize..last as usize),
            || { VMResult::MemoryIndexOutOfRange }
        ));

        slice.fill(vm_try!(VMResult::from_option(data.try_into().ok(), || {
            VMResult::Unreachable
        })));
        VMResult::Success(())
    }
    pub fn copy(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        let src_last = vm_try!(VMResult::from_option(src.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        if src_last > self.0.len() {
            return VMResult::MemoryIndexOutOfRange;
        }
        let dst_last = vm_try!(VMResult::from_option(dst.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        if dst_last > self.0.len() {
            return VMResult::MemoryIndexOutOfRange;
        }
        self.0.copy_within(src as usize..src_last, dst as usize);

        VMResult::Success(())
    }
}
