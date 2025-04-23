#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub struct GcRef(pub u32);

impl GcRef {
    pub fn get(&self) -> u32 {
        self.0
    }
    pub fn get_value_addr(&self) -> u32 {
        self.0 + 1
    }
    pub fn get_usize(&self) -> usize {
        self.0 as usize
    }
    pub fn get_value_addr_usize(&self) -> usize {
        self.get_usize() + 1
    }
}

// GC のトレース用トレイト
pub trait GCView {
    fn trace(&self, tracer: impl FnMut(GcRef));
    fn word_size(&self) -> usize;
}
#[allow(dead_code)]
// マクロ定義
#[macro_export]
macro_rules! gc_object {
    (
        struct $name:ident {
            $( $field_name:ident : $ty:tt ),* $(,)?
        }
    ) => {
        pub struct $name<'a> {
            $(
                pub $field_name: gc_object!(@field_type $ty, 'a),
            )*
            _phantom: ::std::marker::PhantomData<&'a ()>,
        }

        impl<'a> $name<'a> {
            /// # Safety
            /// ptr が正しいレイアウト（u32 単位の連続メモリ）であることを前提とします。
            pub unsafe fn from_ptr(ptr: *const u32) -> Self {
                let base = ptr;
                let mut index = 0;
                $name {
                    $(
                        $field_name: {
                            let v = gc_object!(@read_field base, index, $ty);
                            index += gc_object!(@field_size $ty, v);
                            v
                        },
                    )*
                    _phantom: ::std::marker::PhantomData,
                }
            }
        }

        impl<'a> GCView for $name<'a> {
            fn trace(&self,mut tracer: impl FnMut(GcRef)) {
                $(
                    gc_object!(@trace_field self, $field_name, $ty, tracer);
                )*
            }
            fn word_size(&self) -> usize {
                let mut size = 0;
                $(size += {
                    let v = self.$field_name;
                    gc_object!(@field_size $ty, v)
                };)*
                size
            }
        }
    };

    // ── 型指定 ──
    // 単一フィールド: u32
    (@field_type u32, $lt:lifetime) => { u32 };
    // 単一フィールド: GcRef
    (@field_type GcRef, $lt:lifetime) => { GcRef };
    // スライスフィールド: [u32]
    (@field_type [u32], $lt:lifetime) => { &$lt [u32] };
    // スライスフィールド: [GcRef]
    (@field_type [GcRef], $lt:lifetime) => { &$lt [GcRef] };

    // ── 読み出し処理 ──
    // 単一フィールド: u32
    (@read_field $base:ident, $idx:ident, u32) => {
        *$base.add($idx)
    };
    // 単一フィールド: GcRef → メモリ上の u32 値を GcRef でラップ
    (@read_field $base:ident, $idx:ident, GcRef) => {
        GcRef(*$base.add($idx))
    };
    // スライスフィールド: [u32]
    (@read_field $base:ident, $idx:ident, [u32]) => {{
        let len = *$base.add($idx) as usize;
        ::std::slice::from_raw_parts($base.add($idx + 1), len)
    }};
    // スライスフィールド: [GcRef]
    (@read_field $base:ident, $idx:ident, [GcRef]) => {{
        let len = *$base.add($idx) as usize;
        ::std::slice::from_raw_parts($base.add($idx + 1) as *const GcRef, len)
    }};

    // ── フィールドサイズ（u32 単位） ──
    // 単一フィールド
    (@field_size u32, $v:ident) => { 1 };
    (@field_size GcRef, $v:ident) => { 1 };
    // スライスフィールド
    (@field_size [u32], $s:expr) => { 1 + $s.len() };
    (@field_size [GcRef], $s:expr) => { 1 + $s.len() };

    // ── トレース処理 ──
    // 単一フィールド: u32 → トレースしない
    (@trace_field $self:expr, $field:ident, u32, $tracer: ident) => {};
    // 単一フィールド: GcRef → tracer に GcRef.get() を渡す
    (@trace_field $self:expr, $field:ident, GcRef, $tracer: ident) => {
        $tracer($self.$field);
    };
    // スライスフィールド: [u32] → トレースしない
    (@trace_field $self:expr, $field:ident, [u32], $tracer: ident) => {};
    // スライスフィールド: [GcRef] → 各要素について tracer(item.get())
    (@trace_field $self:expr, $field:ident, [GcRef], $tracer: ident) => {
        for &item in $self.$field.iter() {
            $tracer(item);
        }
    };
}
// マクロ呼び出し例
gc_object! {
  struct InstanceView {
      reference_count: u32,
      module_addr: u32,
      globals: [u32],
      funcs: [u32],
      tables: [u32],
      mems: [u32],
  }
}
#[cfg(test)]
mod tests {
    use crate::common::gc::view::GcRef;

    use super::{GCView, InstanceView};
    #[test]
    fn trace_test() {
        // サンプルデータの生成。ここでは各フィールドを適切なレイアウトでエンコードしています。
        let data: Vec<u32> = {
            let mut v = Vec::new();
            // reference_count: 7
            v.push(7u32);
            // module_addr: 42
            v.push(42u32);
            // globals: len = 3, [1, 2, 3]
            v.push(3u32);
            v.extend_from_slice(&[1u32, 2, 3]);

            // funcs: len = 2, [10, 11]
            v.push(2u32);
            v.extend_from_slice(&[10u32, 11]);

            // tables: len = 3, [100, 101, 102]
            v.push(3u32);
            v.extend_from_slice(&[100u32, 101, 102]);
            v.push(1u32);
            v.extend_from_slice(&[1000u32]);
            v
        };

        // unsafe: 正しいレイアウトであることが前提です
        let instance_view = unsafe { InstanceView::from_ptr(data.as_ptr()) };
        let mut traced = Vec::new();
        // GCView の trace を実行（GcRef 型のフィールドのみがトレース対象となります）
        instance_view.trace(&mut |addr: GcRef| {
            traced.push(addr.get());
        });
        assert_eq!(&traced, &[])
    }
}
