mod catalog;

pub use catalog::Catalog;

pub fn allocate(catalog: &mut Catalog, units: u32) -> Result<(), String> {
    if catalog.can_allocate(units) {
        catalog.record(units)
    } else {
        Err("capacity unavailable".to_owned())
    }
}
