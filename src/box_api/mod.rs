mod client;
mod protocol;
mod server;

pub use client::{BoxApiClient, BoxShell};
pub use protocol::{
    BoxEvent, BoxRequest, BoxResponse, ClientMessage, InteractiveTarget, Principal, ServerMessage,
};
pub use server::{BoxApiServerHandle, serve_box_api_websocket};
