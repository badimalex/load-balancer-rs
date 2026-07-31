use bytes::Bytes;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::HOST;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, StatusCode};
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use simpleload_balancer_rs::LoadBalancer;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;

type HttpClient = Client<HttpConnector, Full<Bytes>>;

async fn handle_request(
    request: Request<Incoming>,
    load_balancer_obj: Arc<Mutex<LoadBalancer>>,
    client: HttpClient,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = request.method();
    let path = request.uri().path();

    match (method, path) {
        (&Method::GET, "/health") => Ok(text_response(StatusCode::OK, "ok")),

        _ => {
            let backend_address: String = {
                let Ok(mut guard) = load_balancer_obj.lock() else {
                    return Ok(text_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal server error",
                    ));
                };

                let Some(next_url) = guard.route() else {
                    return Ok(text_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "no backends available",
                    ));
                };

                next_url.to_owned()
            };
            let path_and_query = request
                .uri()
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/");
            let full_url = format!("{}{}", backend_address, path_and_query);

            let uri: hyper::Uri = match full_url.parse() {
                Ok(uri) => uri,
                Err(_) => {
                    return Ok(text_response(StatusCode::BAD_GATEWAY, "bad gateway"));
                }
            };

            let (mut parts, incoming_body) = request.into_parts();
            parts.uri = uri;
            parts.headers.remove(HOST);

            let collected = match incoming_body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    return Ok(text_response(
                        StatusCode::BAD_REQUEST,
                        "Failed to read the request body",
                    ));
                }
            };

            let full_body = Full::new(collected);

            let upstream_result = timeout(Duration::from_secs(2), async {
                let new_req = Request::from_parts(parts, full_body);
                let res = client.request(new_req).await?;
                let (parts, body) = res.into_parts();
                let bytes = body.collect().await?.to_bytes();
                Ok::<_, Box<dyn std::error::Error>>(Response::from_parts(parts, bytes.into()))
            })
            .await;

            match upstream_result {
                Ok(Ok(response)) => Ok(response),
                /* обработка ошибки запроса */
                Ok(Err(_)) => Ok(text_response(StatusCode::BAD_GATEWAY, "bad gateway")),
                /* обработка истечения времени (Elapsed) */
                Err(_) => Ok(text_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    "gateway timeout",
                )),
            }
        }
    }
}

fn text_response(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

pub async fn run(
    address: &str,
    load_balancer: LoadBalancer,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(address).await?;
    let shared_load_balancer = Arc::new(Mutex::new(load_balancer));
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    loop {
        let client_for_connection = client.clone();

        match listener.accept().await {
            Ok((stream, _)) => {
                let io = TokioIo::new(stream);
                let load_balancer_clone = Arc::clone(&shared_load_balancer);

                tokio::spawn(async move {
                    let service = service_fn(move |req| {
                        let data_clone = Arc::clone(&load_balancer_clone);
                        let client_for_request = client_for_connection.clone();

                        async move { handle_request(req, data_clone, client_for_request).await }
                    });

                    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                        eprintln!("Connection error: {err}");
                    }
                });
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {e}");
            }
        }
    }
}
