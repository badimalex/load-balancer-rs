use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use simpleload_balancer_rs::{Backend, BackendPool, LoadBalancer, http_server::serve};
use tokio::net::TcpListener;
type HttpClient = Client<HttpConnector, Full<Bytes>>;
use hyper_util::rt::TokioExecutor;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;

async fn spawn_proxy(
    load_balancer: LoadBalancer,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // let handle = tokio::spawn(serve(listener, load_balancer));
    let handle = tokio::spawn(async move {
        serve(listener, load_balancer).await.unwrap();
    });

    (addr, handle)
}

#[tokio::test]
async fn test_empty_pool_returns_503() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let pool = BackendPool::new();
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
    let server_handle = tokio::spawn(serve(listener, LoadBalancer::new(pool)));
    let req = Request::builder()
        .uri(format!("http://{}/any", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"no backends available");

    server_handle.abort();
}

async fn spawn_backend(body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    address
}

#[tokio::test]
async fn test_health_does_not_advance_round_robin() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;
    let backend_b = spawn_backend("backend-b").await;

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(Backend::new(format!("http://{backend_b}")));

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
    let load_balancer = LoadBalancer::new(pool);
    let server_handle = tokio::spawn(serve(listener, load_balancer));

    // Запрос /health не должен сдвигать балансировку
    let req_health = Request::builder()
        .method("GET")
        .uri(format!("http://{}/health", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();
    let _ = client.request(req_health).await.unwrap();

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    server_handle.abort();
}

#[tokio::test]
async fn test_health_check() {
    let pool = BackendPool::new();
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .method("GET")
        .uri(format!("http://{}/health", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body_bytes[..], b"ok");

    handle.abort();
}

#[tokio::test]
async fn test_request_forwarded_to_backend() {
    let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_addr = backend_listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = backend_listener.accept().await {
            let response = "HTTP/1.1 201 Created\r\nContent-Length: 16\r\nConnection: close\r\n\r\nbackend-response";
            use tokio::io::AsyncWriteExt;
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    let mut pool = BackendPool::new();
    let backend1 = Backend::new(format!("http://{}", backend_addr));

    pool.add(backend1);

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let body = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"backend-response");

    handle.abort();
}

#[tokio::test]
async fn test_unreachable_backend_returns_502() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr = listener.local_addr().unwrap();
    drop(listener);
    let backend = Backend::new(format!("http://{unavailable_addr}"));

    let mut pool = BackendPool::new();

    pool.add(backend);
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);

    handle.abort();
}
