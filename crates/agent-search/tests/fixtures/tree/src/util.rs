//! Second fixture source file — a regex-mode target (`compute_total`).

pub fn compute_total(items: &[u32]) -> u32 {
    items.iter().sum()
}
