pub struct Catalog {
    remaining: u32,
}

impl Catalog {
    pub fn new(remaining: u32) -> Self {
        Self { remaining }
    }
    pub fn can_allocate(&self, units: u32) -> bool {
        self.remaining >= units
    }
    pub fn record(&mut self, units: u32) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(units)
            .ok_or_else(|| "capacity unavailable".to_owned())?;
        Ok(())
    }
}
