use astra::{Body, Request, Response, Server};

fn main() {
    Server::bind("0.0.0.0:8080")
        .max_workers(num_cpus::get() * 100)
        .serve(serve)
        .expect("failed to start server");
}

fn serve(req: Request) -> Response {
    let (req, _) = req.into_parts();

    let mut headers = req.headers;
    headers.clear();

    let response = simd_json::to_vec(&Json {
        message: "Hello world ...... Hello world ...... ",
        other: 100,
        another: 202020.102,
    })
    .unwrap();

    headers.insert(header::CONTENT_LENGTH, header::TWENTY_SEVEN.clone());
    headers.insert(header::CONTENT_TYPE, header::JSON.clone());
    let body = Body::new(response);

    headers.insert(header::SERVER, header::ASTRA.clone());
    let mut res = Response::new(body);
    *res.headers_mut() = headers;
    res
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Json {
    message: &'static str,
    other: usize,
    another: f64,
}

mod header {
    pub use http::header::*;

    pub static ASTRA: HeaderValue = HeaderValue::from_static("astra");
    pub static TWENTY_SEVEN: HeaderValue = HeaderValue::from_static("27");
    pub static JSON: HeaderValue = HeaderValue::from_static("application/json");
}
