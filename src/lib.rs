pub mod backend;
pub mod http_server;

mod backend_pool;
mod config;
mod errors;
mod load_balancer;

pub use backend::Backend;
pub use backend_pool::BackendPool;
pub use config::Config;
pub use config::ConfigError;
pub use errors::AppError;
pub use load_balancer::LoadBalancer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_exposes_its_address() {
        let backend = Backend::new("server-a:8080".to_string());

        assert_eq!(backend.address(), "server-a:8080");
    }

    #[test]
    fn new_pool_is_empty() {
        let pool = BackendPool::new();

        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());

        let backend = pool.get(0);
        assert!(backend.is_none());
    }

    #[test]
    fn add_updates_pool_len_and_empty_state() {
        let mut pool = BackendPool::new();
        let backend = Backend::new("server-a:8080".to_string());

        pool.add(backend);

        assert_eq!(pool.len(), 1);
        assert!(!pool.is_empty());
    }

    #[test]
    fn get_returns_backend_by_valid_index() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let backend = pool.get(0).expect("backend at index 0 should exist");

        assert_eq!(backend.address(), "server-a:8080");
    }

    #[test]
    fn get_returns_none_for_out_of_bounds_index() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let backend = pool.get(1);

        assert!(backend.is_none());
    }

    #[test]
    fn pool_preserves_insertion_order() {
        let mut pool = BackendPool::new();

        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));

        assert_eq!(pool.get(0).unwrap().address(), "server-a:8080");
        assert_eq!(pool.get(1).unwrap().address(), "server-b:8080");
        assert_eq!(pool.get(2).unwrap().address(), "server-c:8080");
    }

    // RoundRobin

    #[test]
    fn round_robin_cycles_through_backends() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));
        pool.add(Backend::new("server-d:8080".to_string()));

        let mut load_balancer = LoadBalancer::new(pool);

        assert_eq!(load_balancer.route(), Some((0, "server-a:8080")));
        assert_eq!(load_balancer.route(), Some((1, "server-b:8080")));
        assert_eq!(load_balancer.route(), Some((2, "server-c:8080")));
        assert_eq!(load_balancer.route(), Some((3, "server-d:8080")));
        assert_eq!(load_balancer.route(), Some((0, "server-a:8080")));
        assert_eq!(load_balancer.route(), Some((1, "server-b:8080")));
        assert_eq!(load_balancer.route(), Some((2, "server-c:8080")));
        assert_eq!(load_balancer.route(), Some((3, "server-d:8080")));
    }

    #[test]
    fn single_backend_is_selected_repeatedly() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let mut lb = LoadBalancer::new(pool);

        for _ in 0..5 {
            assert_eq!(lb.route(), Some((0, "server-a:8080")));
        }
    }

    #[test]
    fn empty_pool_returns_none() {
        let pool = BackendPool::new();
        let mut selector = LoadBalancer::new(pool);

        assert_eq!(selector.route(), None);
    }

    #[test]
    fn load_balancer_instances_have_independent_routing_state() {
        let mut pool1 = BackendPool::new();
        pool1.add(Backend::new("a".to_string()));
        pool1.add(Backend::new("b".to_string()));
        pool1.add(Backend::new("c".to_string()));
        let mut lb1 = LoadBalancer::new(pool1);

        let mut pool2 = BackendPool::new();
        pool2.add(Backend::new("a".to_string()));
        pool2.add(Backend::new("b".to_string()));
        pool2.add(Backend::new("c".to_string()));
        let mut lb2 = LoadBalancer::new(pool2);

        assert_eq!(lb1.route(), Some((0, "a")));
        assert_eq!(lb1.route(), Some((1, "b")));

        assert_eq!(lb2.route(), Some((0, "a")));

        assert_eq!(lb1.route(), Some((2, "c")));
        assert_eq!(lb2.route(), Some((1, "b")));
        assert_eq!(lb2.route(), Some((2, "c")));
    }

    #[test]
    fn new_backend_is_healthy() {
        let b = Backend::new("127.0.0.1:8080".to_string());
        assert!(b.is_healthy());
    }

    #[test]
    fn backend_can_be_marked_unhealthy() {
        let mut b = Backend::new("127.0.0.1:8080".to_string());
        b.set_healthy(false);
        assert!(!b.is_healthy());
    }

    #[test]
    fn backend_can_become_healthy_again() {
        let mut b = Backend::new("127.0.0.1:8080".to_string());
        b.set_healthy(false);
        b.set_healthy(true);
        assert!(b.is_healthy());
    }

    #[test]
    fn routing_skips_several_unhealthy_backends() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server1".to_string()));
        pool.add(Backend::new("server2".to_string()));
        pool.add(Backend::new("server3".to_string()));
        pool.add(Backend::new("server4".to_string()));

        let mut lb = LoadBalancer::new(pool);
        lb.set_backend_healthy(1, false);
        lb.set_backend_healthy(2, false);

        assert_eq!(lb.route(), Some((0, "server1")));
        assert_eq!(lb.route(), Some((3, "server4"))); // пропуск 2 и 3
        assert_eq!(lb.route(), Some((0, "server1")));
    }

    #[test]
    fn all_backends_unhealthy_returns_none() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server1".to_string()));
        pool.add(Backend::new("server2".to_string()));

        let mut lb = LoadBalancer::new(pool);
        lb.set_backend_healthy(0, false);
        lb.set_backend_healthy(1, false);
        assert_eq!(lb.route(), None);
    }

    #[test]
    fn one_healthy_backend_is_selected_repeatedly() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server1".to_string()));
        pool.add(Backend::new("server2".to_string()));
        pool.add(Backend::new("server3".to_string()));

        let mut lb = LoadBalancer::new(pool);
        lb.set_backend_healthy(0, false);
        lb.set_backend_healthy(2, false);

        assert_eq!(lb.route(), Some((1, "server2")));
        assert_eq!(lb.route(), Some((1, "server2")));
        assert_eq!(lb.route(), Some((1, "server2")));
    }

    #[test]
    fn recovered_backend_returns_to_rotation() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server1".to_string()));
        pool.add(Backend::new("server2".to_string()));

        let mut lb = LoadBalancer::new(pool);
        lb.set_backend_healthy(1, false);

        assert_eq!(lb.route(), Some((0, "server1")));
        assert_eq!(lb.route(), Some((0, "server1")));

        // Восстанавливаем server2
        lb.set_backend_healthy(1, true);

        assert_eq!(lb.route(), Some((1, "server2")));
        assert_eq!(lb.route(), Some((0, "server1")));
    }

    #[test]
    fn backend_marked_unhealthy_through_load_balancer_is_skipped() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server_A".to_string()));
        pool.add(Backend::new("server_B".to_string()));
        pool.add(Backend::new("server_C".to_string()));

        let mut lb = LoadBalancer::new(pool);

        lb.set_backend_healthy(1, false);

        assert_eq!(lb.route(), Some((0, "server_A")));
        assert_eq!(lb.route(), Some((2, "server_C")));
        assert_eq!(lb.route(), Some((0, "server_A")));
        assert_eq!(lb.route(), Some((2, "server_C")));
    }

    #[test]
    fn test_unhealthy_backends_snapshot() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server_A".to_string()));
        pool.add(Backend::new("server_B".to_string()));
        pool.add(Backend::new("server_C".to_string()));

        let mut lb = LoadBalancer::new(pool);

        let snapshot = lb.unhealthy_backends_snapshot();
        assert_eq!(snapshot.len(), 0);

        lb.set_backend_healthy(1, false);
        let snapshot2 = lb.unhealthy_backends_snapshot();
        assert_eq!(snapshot2.len(), 1);

        assert_eq!(snapshot2[0].index, 1);
        assert_eq!(snapshot2[0].address, "server_B");
    }
}
