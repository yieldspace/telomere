use std::mem::size_of;

/// フレームヘッダ。引数領域とローカル領域のサイズ、およびフレーム生成前のスタックトップ位置を保持する。
#[repr(C)]
struct FrameHeader {
    argument_size: usize,
    local_size: usize,
    prev_top: usize,
}

/// 固定サイズバッファ上に構築するコールスタック。
pub struct CallStack {
    memory: Box<[u8]>,
    top: usize, // 現在のスタックトップ（オフセット）
}

impl CallStack {
    /// 指定したバイト数のメモリを確保して、コールスタックを作成する。
    pub fn new(capacity: usize) -> Self {
        let mut vec = Vec::with_capacity(capacity);
        vec.resize(capacity, 0);
        Self {
            memory: vec.into_boxed_slice(),
            top: 0,
        }
    }

    /// 引数領域とローカル領域のサイズを指定して、新たなスタックフレームをプッシュする。
    /// 成功すればフレームハンドルを返す（失敗すれば None）。
    pub fn push_frame(&mut self, argument_size: usize, local_size: usize) -> Option<Frame<'_>> {
        let header_size = size_of::<FrameHeader>();
        let required = header_size + argument_size + local_size;
        if self.top + required > self.memory.len() {
            return None;
        }
        // ヘッダの書き込み
        let header_ptr = unsafe { self.memory.as_mut_ptr().add(self.top) as *mut FrameHeader };
        unsafe {
            (*header_ptr).argument_size = argument_size;
            (*header_ptr).local_size = local_size;
            (*header_ptr).prev_top = self.top;
        }
        let frame_base = self.top;
        self.top += required;
        Some(Frame {
            stack: self,
            base: frame_base,
            header_size,
            argument_size,
            local_size,
            local_stack_top: 0,
        })
    }
}

/// 各スタックフレームを表すハンドル。スコープを抜ける際に Drop され、自動的にスタックがポップされる。
pub struct Frame<'a> {
    stack: &'a mut CallStack,
    base: usize,          // このフレームの開始オフセット
    header_size: usize,   // ヘッダ部のサイズ（固定）
    argument_size: usize, // 引数領域のサイズ
    local_size: usize,    // ローカル領域のサイズ（拡張可能）
    local_stack_top: usize, // ローカル領域内での push/pop 用のスタックポインタ
}

impl<'a> Frame<'a> {
    /// フレームの引数領域へのミュータブルな参照を取得する。
    pub fn argument_area(&mut self) -> &mut [u8] {
        let start = self.base + self.header_size;
        let end = start + self.argument_size;
        &mut self.stack.memory[start..end]
    }

    /// フレームのローカル領域へのミュータブルな参照を取得する。
    pub fn local_area(&mut self) -> &mut [u8] {
        let start = self.base + self.header_size + self.argument_size;
        let end = start + self.local_size;
        &mut self.stack.memory[start..end]
    }

    /// ローカル領域に固定長配列 [u8; N] を push する関数。
    /// 十分な空き領域がなければエラーを返します。
    pub fn push_u8_array<const N: usize>(&mut self, data: [u8; N]) -> Result<(), &'static str> {
        if self.local_stack_top + N > self.local_size {
            return Err("ローカル領域が不足しています");
        }
        let start = self.base + self.header_size + self.argument_size;
        let dest_index = self.local_stack_top;
        let dest_range = start + dest_index .. start + dest_index + N;
        self.stack.memory[dest_range].copy_from_slice(&data);
        self.local_stack_top += N;
        Ok(())
    }

    /// ローカル領域から固定長配列 [u8; N] を pop する関数。
    /// 十分なデータがなければ None を返します。
    pub fn pop_u8_array<const N: usize>(&mut self) -> Option<[u8; N]> {
        if self.local_stack_top < N {
            return None;
        }
        let start = self.base + self.header_size + self.argument_size + self.local_stack_top - N;
        let mut arr = [0u8; N];
        arr.copy_from_slice(&self.stack.memory[start .. start + N]);
        self.local_stack_top -= N;
        Some(arr)
    }

    /// 現在のトップフレームである場合に、ローカル領域を additional バイトだけ拡張する。
    /// 拡張に成功すれば true を返します。
    pub fn extend_local(&mut self, additional: usize) -> bool {
        let current_end = self.base + self.header_size + self.argument_size + self.local_size;
        if current_end != self.stack.top {
            return false;
        }
        if self.stack.top + additional > self.stack.memory.len() {
            return false;
        }
        self.local_size += additional;
        // ヘッダ内のローカル領域サイズを更新
        unsafe {
            let header_ptr = self.stack.memory.as_mut_ptr().add(self.base) as *mut FrameHeader;
            (*header_ptr).local_size = self.local_size;
        }
        self.stack.top += additional;
        true
    }
}

impl<'a> Drop for Frame<'a> {
    /// フレームがスコープを抜ける際に、スタックをこのフレーム生成前の状態に戻す（ポップする）。
    fn drop(&mut self) {
        self.stack.top = self.base;
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::core::stack::CallStack;

    #[test]
    fn test_call_stack() {
        let mut stack = CallStack::new(1024);
        let mut frame = stack.push_frame(0, 10).unwrap();
        let data = [10, 20, 30, 40, 50];
        frame.push_u8_array(data).unwrap();
        assert_eq!(frame.pop_u8_array::<5>().unwrap(), [10, 20, 30, 40, 50]);
    }
}
