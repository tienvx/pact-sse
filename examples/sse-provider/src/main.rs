use actix_web::{web, App, HttpResponse, HttpServer};
use log::*;

async fn get_events() -> HttpResponse {
    debug!("GET request for SSE events");

    let sse_data = "id:901
retry:20

data: I read this book many times

event:count
data:34

data: Last time I read it

event:time
data:2015-02-21

event:user
data: id: 12, name: John

".to_string();

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("X-Accel-Buffering", "no"))
        .body(sse_data)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = simple_log::quick();
    info!("Starting SSE provider on 127.0.0.1:8080");

    HttpServer::new(|| {
        App::new()
            .route("/events", web::get().to(get_events))
            .route(
                "/pact-change-state",
                web::post().to(|| async { HttpResponse::Ok().finish() }),
            )
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
