use seam::Store;
use external::Unknown;

pub struct Memory;
impl seam::Store for Memory {
    async fn read(&self) -> String { "memory".into() }
}
impl Memory {
    pub fn new() -> Self { Self }
}
