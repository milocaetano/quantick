#[cfg(any(test, feature = "fake"))]
pub fn fake() -> u8 {
    Some(1).unwrap()
}
#[cfg(all(unix, test))]
impl Late {
    fn late() -> u8 { 2 }
}
#[cfg(not(test))]
pub fn live() {
}
#[cfg(test)]
#[allow(dead_code)]
impl Fixture {
}
impl Live {
}
