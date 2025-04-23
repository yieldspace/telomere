use crate::common::word_size;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Raw = 1,
    Instance = 2,
    RootTable = 3,
    ExternMemoryRef = 4,
    ExternTableRef = 5,
    ExternModuleRef = 6,
    GlobalRef = 7,
    FunctionInstance = 8,
}
pub(crate) const PADDING_MASK: u32 = 1 << 31;
const INIT_MASK: u32 = 1 << 30;
const MARK_MASK: u32 = 1 << 29;
const ALIGN64_MASK: u32 = 1 << 28;
const NEED_PADDING_MASK: u32 = 1 << 27;
const UNNEED_PADDING_MASK: u32 = !NEED_PADDING_MASK;
const UNMARK_MASK: u32 = !MARK_MASK;
const SIZE_MASK: u32 = 0xFFFF;
const TYPE_MASK: u32 = 0xFF;
const TYPE_LOWER_BIT: u32 = 16;
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Header(u32, u32);
impl Header {
    pub fn object_type(&self) -> ObjectType {
        unsafe { std::mem::transmute((self.0 >> TYPE_LOWER_BIT) & TYPE_MASK) }
    }
    pub fn word_size(&self) -> u16 {
        (self.0 & SIZE_MASK) as u16
    }
    pub fn is_marked(&self) -> bool {
        self.0 & MARK_MASK != 0
    }
    pub fn is_align64(self) -> bool {
        self.0 & ALIGN64_MASK != 0
    }
    pub fn is_padding(self) -> bool {
        self.0 & PADDING_MASK != 0
    }
    pub fn is_need_padding(self) -> bool {
        self.0 & NEED_PADDING_MASK != 0
    }
    pub fn marked(self) -> Self {
        Self(self.0 | MARK_MASK, self.1)
    }
    pub fn unmarked(self) -> Self {
        Self(self.0 & UNMARK_MASK, self.1)
    }
    pub fn initialized(self) -> Self {
        Self(self.0 | INIT_MASK, self.1)
    }
    pub fn align64(self) -> Self {
        Self(self.0 | ALIGN64_MASK, self.1)
    }
    pub fn need_padding(self) -> Self {
        Self(self.0 | NEED_PADDING_MASK, self.1)
    }
    pub fn unneed_padding(self) -> Self {
        Self(self.0 & UNNEED_PADDING_MASK, self.1)
    }
    pub fn set_forwarding_pointer(self, ptr: u32) -> Self {
        Self(self.0, ptr)
    }
    pub fn forwarding_pointer(self) -> u32 {
        self.1
    }
    pub fn is_initialized(&self) -> bool {
        self.0 & INIT_MASK != 0
    }
    pub fn new(ty: ObjectType, size: usize) -> Self {
        let ty: u32 = unsafe { std::mem::transmute(ty) };
        if size > u16::MAX.into() {
            panic!()
        }
        Self(ty << TYPE_LOWER_BIT | (size as u32), 0)
    }
    pub fn get(&self) -> [u32; 2] {
        [self.0, self.1]
    }
    pub(crate) fn from_raw(a: u32, b: u32) -> Self {
        Self(a, b)
    }
}
pub const HEADER_LEN: usize = word_size::<Header>();
