#[derive(Debug)]
pub struct Backend {
    // private
    address: String,
    healthy: bool,
}

impl Backend {
    pub fn new(address: String) -> Self {
        Self {
            address,
            healthy: true,
        }
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy
    }

    pub fn set_healthy(&mut self, healthy: bool) {
        self.healthy = healthy;
    }
}
