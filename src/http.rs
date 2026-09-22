//! Minimal HTTP server hosted on the chip.
//!
//! [`Http::start`] takes the embassy-net [`Stack`] handed over by [`crate::wifi`]
//! and spawns a task that serves requests for the lifetime of the program.

use embassy_executor::Spawner;
use embassy_net::Stack;
use picoserve::routing::get;
use picoserve::{Config, Router, Server};
use static_cell::StaticCell;

/// TCP port the web server listens on.
pub const PORT: u16 = 80;

/// Scratch buffer used to parse/write HTTP requests and responses.
const HTTP_BUFFER_SIZE: usize = 2048;

/// Per-connection TCP receive/transmit buffers.
const TCP_BUFFER_SIZE: usize = 2048;

/// Everything the HTTP server needs, kept alive for as long as the chip runs.
///
/// The picoserve [`Router`] cannot be stored here because its type is opaque
/// (`Router<impl PathRouter>`); it is built inside [`server_task`] instead.
pub struct Http {
    stack: Stack<'static>,
    config: Config,
    http_buffer: [u8; HTTP_BUFFER_SIZE],
    tcp_rx_buffer: [u8; TCP_BUFFER_SIZE],
    tcp_tx_buffer: [u8; TCP_BUFFER_SIZE],
}

static HTTP: StaticCell<Http> = StaticCell::new();

impl Http {
    fn new(stack: Stack<'static>) -> Self {
        Self {
            stack,
            config: Config::const_default(),
            http_buffer: [0; HTTP_BUFFER_SIZE],
            tcp_rx_buffer: [0; TCP_BUFFER_SIZE],
            tcp_tx_buffer: [0; TCP_BUFFER_SIZE],
        }
    }

    /// Store the server state in static memory and spawn its task.
    pub fn start(spawner: Spawner, stack: Stack<'static>) {
        let http = HTTP.init_with(|| Http::new(stack));
        spawner.spawn(server_task(http).expect("spawn http task"));
    }
}

#[embassy_executor::task]
async fn server_task(http: &'static mut Http) {
    let app = Router::new().route("/", get(|| async { "Hello World" }));

    Server::new(&app, &http.config, &mut http.http_buffer)
        .listen_and_serve(
            "http",
            http.stack,
            PORT,
            &mut http.tcp_rx_buffer,
            &mut http.tcp_tx_buffer,
        )
        .await;
}
