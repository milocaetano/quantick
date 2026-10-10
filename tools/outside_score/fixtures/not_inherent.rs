impl std::fmt::Display for Hub {
}
impl<T> From<T> for Hub {
}
unsafe impl Send for Hub {}
pub fn take(hub: impl Into<Hub>) {}
pub fn make() -> impl Fn() -> u8 {
    || 1
}
impl<F: Fn() -> u8> Run for Task<F> {
}
// impl Hub {
/* impl Hub { */
const TEXT: &str = "
impl Hub {
";
