// Server Imports

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
    Json, Router,
};

use serde::{Deserialize, Serialize};
use serde_json::json;

// Runtime imports

use tokio::net::TcpListener;

// Functional Imports

use gold_2::Accounts;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    // Set up blockchain state

    let accounts: Accounts = HashMap::new();

    // Set up tcp connection

    let listener = TcpListener::bind("127.0.0.1:9280")
        .await
        .expect("Could not create TCP Listener");

    // Compose routes

    let app = Router::new().route("/", get(|| async { "Hello, World!" }));

    // Serve the application

    axum::serve(listener, app)
        .await
        .expect("Error serving application")
}
