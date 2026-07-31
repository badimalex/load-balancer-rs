use simpleload_balancer_rs::{Backend, BackendPool, LoadBalancer};

mod http_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut pool = BackendPool::new();

    pool.add(Backend::new("http://127.0.0.1:4001".to_string()));
    pool.add(Backend::new("http://127.0.0.1:4002".to_string()));

    let load_balancer = LoadBalancer::new(pool);

    http_server::run("127.0.0.1:3000", load_balancer).await
}
