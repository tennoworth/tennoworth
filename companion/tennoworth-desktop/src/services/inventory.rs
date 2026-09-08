use std::sync::OnceLock;
use wfm_core::acquisition::inventory::InventoryScanner;

/// Process-wide scanner so the single-flight guard actually serializes two
/// concurrent `scan_inventory` invokes (a second concurrent scan gets
/// ScanError::Busy rather than a redundant parallel walk of the address space).
pub(crate) fn scanner() -> &'static InventoryScanner {
    static SCANNER: OnceLock<InventoryScanner> = OnceLock::new();
    SCANNER.get_or_init(InventoryScanner::new)
}
