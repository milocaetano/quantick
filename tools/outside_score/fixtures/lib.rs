//! A crate. The word fn in a comment is not a function. {
use std::fmt;

pub(crate) fn tricky<'a>(s: &'a str) -> usize {
    let raw = r#"fn fake() { "quoted" }"#; let c = '{'; let q = '\'';
    let b = b'}'; let text = "unbalanced { brace and fn inside";
    s.len() + raw.len() + text.len() + (c as usize) + (q as usize) + (b as usize)
}

/* block /* nested { */ comment } */
pub(super) fn short() -> u8 {
    let v: Option<u8> = None;
    v.unwrap()
}

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Square;

impl Square {
    pub fn side(&self) -> f64 { 1.0 }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "sq") }
}

pub fn evens() -> impl Iterator<Item = u8> {
    (0..4).filter(|n| n % 2 == 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_is_not_production() {
        let _ = "QUANTICK_TEST_ONLY";
        panic!("never counted");
    }
}
