//! Corpus module A for the #188 vm-profile frequency report.
//!
//! A small but real compiled workload: a hand-rolled JSON parser over an
//! embedded document, aggregating numeric fields per key into a hash map,
//! then folding the map into a checksum. Built with rustc -O for
//! wasm32-unknown-unknown; the exported `run` repeats the parse `n` times
//! and returns the checksum so the result is observable (and identical
//! across engines).

use std::collections::HashMap;

const DOC: &str = r#"{
  "users": [
    {"id": 1, "name": "ada", "score": 91.5, "tags": ["admin", "ops"], "active": true},
    {"id": 2, "name": "brian", "score": 72.25, "tags": ["dev"], "active": false},
    {"id": 3, "name": "cy", "score": 88.0, "tags": ["dev", "qa"], "active": true},
    {"id": 4, "name": "dee", "score": 64.75, "tags": [], "active": true},
    {"id": 5, "name": "eli", "score": 79.5, "tags": ["ops"], "active": false}
  ],
  "meta": {"count": 5, "region": "ap-northeast-1", "ratio": 0.625}
}"#;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self { bytes: text.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> u8 {
        self.bytes[self.pos]
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && (self.bytes[self.pos] as char).is_whitespace() {
            self.pos += 1;
        }
    }

    fn parse_string(&mut self) -> String {
        assert_eq!(self.peek(), b'"');
        self.pos += 1;
        let start = self.pos;
        while self.peek() != b'"' {
            self.pos += 1;
        }
        let out = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
        self.pos += 1;
        out
    }

    fn parse_number(&mut self) -> f64 {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b'0'..=b'9' | b'-' | b'.' | b'e' | b'E' | b'+')
        {
            self.pos += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .unwrap()
            .parse::<f64>()
            .unwrap()
    }

    /// Sums every numeric value, keyed by the most recent object key.
    fn parse_value(&mut self, key: &str, sums: &mut HashMap<String, f64>, counts: &mut HashMap<String, u32>) {
        self.skip_ws();
        match self.peek() {
            b'{' => {
                self.pos += 1;
                loop {
                    self.skip_ws();
                    if self.peek() == b'}' {
                        self.pos += 1;
                        return;
                    }
                    let child_key = self.parse_string();
                    self.skip_ws();
                    assert_eq!(self.peek(), b':');
                    self.pos += 1;
                    self.parse_value(&child_key, sums, counts);
                    self.skip_ws();
                    if self.peek() == b',' {
                        self.pos += 1;
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                loop {
                    self.skip_ws();
                    if self.peek() == b']' {
                        self.pos += 1;
                        return;
                    }
                    self.parse_value(key, sums, counts);
                    self.skip_ws();
                    if self.peek() == b',' {
                        self.pos += 1;
                    }
                }
            }
            b'"' => {
                let text = self.parse_string();
                *counts.entry(key.to_string()).or_insert(0) += text.len() as u32;
            }
            b't' => {
                self.pos += 4;
                *sums.entry(key.to_string()).or_insert(0.0) += 1.0;
            }
            b'f' => {
                self.pos += 5;
            }
            b'n' => {
                self.pos += 4;
            }
            _ => {
                let value = self.parse_number();
                *sums.entry(key.to_string()).or_insert(0.0) += value;
                *counts.entry(key.to_string()).or_insert(0) += 1;
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn run(n: i32) -> i32 {
    let mut checksum: u64 = 0;
    for _ in 0..n.max(1) {
        let mut sums: HashMap<String, f64> = HashMap::new();
        let mut counts: HashMap<String, u32> = HashMap::new();
        let mut parser = Parser::new(DOC);
        parser.parse_value("", &mut sums, &mut counts);
        let mut keys: Vec<&String> = sums.keys().collect();
        keys.sort();
        for key in keys {
            checksum = checksum.wrapping_mul(31).wrapping_add(sums[key].to_bits());
            checksum ^= counts.get(key).copied().unwrap_or(0) as u64;
        }
    }
    (checksum & 0x7fff_ffff) as i32
}
