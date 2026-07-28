use crate::BackendPool;

#[derive(Default, Debug)]
pub struct RoundRobin {
    // private
    current: usize,
}

impl RoundRobin {
    pub fn new() -> Self {
        Self { current: 0 }
    }

    pub fn select_next(&mut self, pool: &BackendPool) -> Option<usize> {
        if pool.is_empty() {
            return None;
        }

        let current = self.current;
        self.current = if self.current + 1 == pool.len() {
            0
        } else {
            current + 1
        };

        Some(current)
    }
}
