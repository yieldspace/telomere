//! Corpus module B for the #188 vm-profile frequency report.
//!
//! A small but real compiled workload with a different instruction mix
//! from module A: it tokenizes an embedded text, builds an inverted index
//! (term -> posting list) with a hash map, sorts terms by frequency, and
//! answers a fixed set of lookups, folding everything into a checksum.
//! Built with rustc -O for wasm32-unknown-unknown; the exported `run`
//! repeats the build `n` times and returns the checksum.

use std::collections::HashMap;

const TEXT: &str = "the quick brown fox jumps over the lazy dog \
the dog barked at the fox and the fox ran over the hill \
a quick fox is a clever fox and a lazy dog is a sleepy dog \
the hill was green and the sky was blue over the quiet town \
the town watched the fox and the dog rest under the tree";

fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

#[no_mangle]
pub extern "C" fn run(n: i32) -> i32 {
    let mut checksum: u64 = 0;
    for round in 0..n.max(1) {
        let words = tokenize(TEXT);
        let mut index: HashMap<&str, Vec<u32>> = HashMap::new();
        for (position, word) in words.iter().enumerate() {
            index.entry(word).or_default().push(position as u32);
        }
        let mut ranked: Vec<(&str, usize)> =
            index.iter().map(|(term, postings)| (*term, postings.len())).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        for (term, frequency) in &ranked {
            checksum = checksum
                .wrapping_mul(131)
                .wrapping_add((*frequency as u64) << 8)
                .wrapping_add(term.len() as u64);
        }
        for probe in ["fox", "dog", "the", "hill", "absent"] {
            let postings = index.get(probe).map(|list| list.len() as u64).unwrap_or(0);
            checksum ^= postings.wrapping_mul(0x9e37_79b9).rotate_left(round as u32 % 31);
        }
        checksum = checksum.wrapping_add(words.len() as u64);
    }
    (checksum & 0x7fff_ffff) as i32
}
