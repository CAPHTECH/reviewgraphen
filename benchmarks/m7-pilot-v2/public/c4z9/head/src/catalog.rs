pub struct Catalog {
    remaining: u32,
}

impl Catalog {
    pub fn new(remaining: u32) -> Self {
        Self { remaining }
    }
    pub fn can_allocate(&mut self, units: u32) -> bool {
        let available = self.remaining >= units;
        available
    }
    pub fn record(&mut self, units: u32) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(units)
            .ok_or_else(|| "capacity unavailable".to_owned())?;
        Ok(())
    }
}
