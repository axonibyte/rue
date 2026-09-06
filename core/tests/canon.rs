//! The canonical byte encoding: exact bytes on fixed values, and the
//! collisions the tags exist to prevent.

use rue_core::canon::{message, Canon, Encoder};

struct Rec;
impl Canon for Rec {
    fn canon(&self, e: &mut Encoder) {
        e.record(4);
        e.u64(7);
        e.str("ab");
        e.option(&Some(true));
        e.list(&["x".to_string()]);
    }
}

#[test]
fn exact_bytes_of_a_fixed_message() {
    let bytes = message("dom", &Rec);
    let expected: Vec<u8> = [
        vec![0x05, 0, 0, 0, 0, 0, 0, 0, 3, b'd', b'o', b'm'],
        vec![0x07, 0, 0, 0, 0, 0, 0, 0, 4],
        vec![0x03, 0, 0, 0, 0, 0, 0, 0, 7],
        vec![0x05, 0, 0, 0, 0, 0, 0, 0, 2, b'a', b'b'],
        vec![0x01, 0x02, 0x01],
        vec![
            0x06, 0, 0, 0, 0, 0, 0, 0, 1, 0x05, 0, 0, 0, 0, 0, 0, 0, 1, b'x',
        ],
    ]
    .concat();
    assert_eq!(bytes, expected);
}

#[test]
fn none_empty_string_and_empty_list_never_collide() {
    let none = message("d", &None::<String>);
    let empty = message("d", &String::new());
    let list = message("d", &Vec::<String>::new());
    let some_empty = message("d", &Some(String::new()));
    assert_ne!(none, empty);
    assert_ne!(none, list);
    assert_ne!(empty, list);
    assert_ne!(some_empty, empty);
}

#[test]
fn domains_and_field_order_separate_messages() {
    struct Ab;
    impl Canon for Ab {
        fn canon(&self, e: &mut Encoder) {
            e.record(2);
            e.str("a");
            e.str("b");
        }
    }
    struct Ba;
    impl Canon for Ba {
        fn canon(&self, e: &mut Encoder) {
            e.record(2);
            e.str("b");
            e.str("a");
        }
    }
    assert_ne!(message("one", &Ab), message("two", &Ab));
    assert_ne!(message("one", &Ab), message("one", &Ba));
    // Adjacent strings cannot re-split: "ab","c" is not "a","bc".
    struct S1;
    impl Canon for S1 {
        fn canon(&self, e: &mut Encoder) {
            e.record(2);
            e.str("ab");
            e.str("c");
        }
    }
    struct S2;
    impl Canon for S2 {
        fn canon(&self, e: &mut Encoder) {
            e.record(2);
            e.str("a");
            e.str("bc");
        }
    }
    assert_ne!(message("d", &S1), message("d", &S2));
}
