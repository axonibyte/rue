//! The canonical printer: exact bytes on fixed cases, and a round trip on
//! generated values (encode, parse, re-encode: same bytes; and the parse is
//! the original value). A port of the prototype's Test.Canonical, with the
//! same expected bytes written out rather than computed.

use rue_core::json::canonical::{encode, CanonicalError};
use serde_json::{json, Value};

#[test]
fn empty_object_and_array() {
    assert_eq!(encode(&json!({})).unwrap(), b"{}\n");
    assert_eq!(encode(&json!([])).unwrap(), b"[]\n");
}

#[test]
fn keys_sorted_by_code_point_two_space_indent() {
    let v = json!({"b": 2, "a": 1, "Z": true});
    assert_eq!(
        encode(&v).unwrap(),
        b"{\n  \"Z\": true,\n  \"a\": 1,\n  \"b\": 2\n}\n"
    );
}

#[test]
fn nested_containers_one_element_per_line() {
    let v = json!({"xs": [1, {"k": null}]});
    assert_eq!(
        encode(&v).unwrap(),
        b"{\n  \"xs\": [\n    1,\n    {\n      \"k\": null\n    }\n  ]\n}\n"
    );
}

#[test]
fn escaping_quote_backslash_named_controls_other_controls_raw_non_ascii() {
    let v = Value::String("a\"b\\c\nd\t\u{1}\u{e9}\u{2013}".to_string());
    // U+00E9 and U+2013 stay raw UTF-8; written out so the test does not
    // compute them with the code under test.
    let mut expected: Vec<u8> = b"\"a\\\"b\\\\c\\nd\\t\\u0001".to_vec();
    expected.extend_from_slice(&[0xC3, 0xA9, 0xE2, 0x80, 0x93]);
    expected.extend_from_slice(b"\"\n");
    assert_eq!(encode(&v).unwrap(), expected);
}

#[test]
fn non_integer_numbers_are_refused_with_their_path() {
    let v = json!({"a": [1, {"b": 1.5}]});
    assert_eq!(
        encode(&v),
        Err(CanonicalError::NonInteger {
            path: "$.a[1].b".to_string()
        })
    );
}

#[test]
fn round_trip_over_generated_values() {
    // xorshift32: deterministic across platforms, replayable from the seed.
    struct Rng(u32);
    impl Rng {
        fn next(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            x
        }
        fn below(&mut self, n: u32) -> u32 {
            self.next() % n
        }
    }
    // Keys drawn from a set that exercises code-point ordering (digits, upper,
    // lower, punctuation); strings from letters, the two escaped ASCII
    // characters, named and unnamed controls, and raw non-ASCII.
    const KEY_CHARS: &[char] = &['0', '9', 'A', 'Z', 'a', 'z', '_', '-', '.'];
    const STR_CHARS: &[char] = &[
        'a',
        'q',
        'z',
        '"',
        '\\',
        ' ',
        '\n',
        '\r',
        '\t',
        '\u{8}',
        '\u{c}',
        '\u{1}',
        '\u{1f}',
        '\u{e9}',
        '\u{2013}',
        '\u{1f600}',
    ];
    fn gen(rng: &mut Rng, depth: u32) -> Value {
        let leaf = depth == 0 || rng.below(2) == 0;
        if leaf {
            match rng.below(4) {
                0 => Value::Null,
                1 => Value::Bool(rng.below(2) == 0),
                2 => Value::from(rng.next() as i64 - (u32::MAX / 2) as i64),
                _ => {
                    let n = rng.below(6);
                    Value::String(
                        (0..n)
                            .map(|_| STR_CHARS[rng.below(STR_CHARS.len() as u32) as usize])
                            .collect(),
                    )
                }
            }
        } else if rng.below(3) == 0 {
            let n = rng.below(5);
            Value::Array((0..n).map(|_| gen(rng, depth - 1)).collect())
        } else {
            let n = rng.below(5);
            let mut m = serde_json::Map::new();
            for _ in 0..n {
                let klen = 1 + rng.below(3);
                let k: String = (0..klen)
                    .map(|_| KEY_CHARS[rng.below(KEY_CHARS.len() as u32) as usize])
                    .collect();
                m.insert(k, gen(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
    let mut rng = Rng(0x9E37_79B9);
    for _ in 0..500 {
        let v = gen(&mut rng, 3);
        let bytes = encode(&v).unwrap_or_else(|e| panic!("{e} for {v}"));
        let back: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("unparseable ({e}): {}", String::from_utf8_lossy(&bytes)));
        assert_eq!(back, v);
        assert_eq!(encode(&back).unwrap(), bytes);
    }
}
