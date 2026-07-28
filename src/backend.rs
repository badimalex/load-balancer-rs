#[derive(Default, Debug)]
pub struct Backend {
    // private
    address: String,
}

impl Backend {
    pub fn new(address: String) -> Self {
        Self { address }
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}
