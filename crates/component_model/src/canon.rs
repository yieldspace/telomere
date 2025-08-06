pub struct CanonOpts {
    pub string_encoding: StringEncoding,
}

pub enum StringEncoding {
    Utf8,
    Utf16,
    Latin1Utf16,
}
