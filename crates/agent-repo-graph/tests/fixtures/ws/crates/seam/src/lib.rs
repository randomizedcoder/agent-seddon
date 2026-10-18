//! A fixture contract.
#[async_trait]
pub trait Store {
    async fn read(&self) -> String;
}

pub struct Record { pub value: u32 }
pub enum State { Ready, Failed }
pub type Count = u64;
pub const LIMIT: usize = 10;
pub static NAME: &str = "fixture";
macro_rules! local_macro { () => { 1 }; }

/// Read a record.
pub fn read() -> u32 { 1 }

#[cfg(feature = "fast")]
pub fn selected() -> u32 { 1 }
#[cfg(feature = "slow")]
pub fn selected() -> u32 { 2 }

#[cfg(not(feature = "slow"))]
pub fn non_slow() {}

#[path = "special.rs"]
pub mod renamed;
pub mod nested;

#[cfg(test)]
mod tests {
    #[rstest]
    #[case::positive_value(1)]
    #[case::negative_value(0)]
    #[case::corner_value(0)]
    #[case::boundary_value(10)]
    #[case::adversarial_value(99)]
    fn read_cases(#[case] value: u32) { assert!(value <= 99); }

    #[tokio::test]
    #[ignore = "requires external service"]
    async fn positive_read() { assert_eq!(super::read(), 1); }
}
