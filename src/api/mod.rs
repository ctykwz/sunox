//! Suno web endpoint client, endpoint methods, auth retry, and response mapping.

pub mod billing;
pub mod challenge;
pub mod concat;
pub mod cover;
pub mod delete;
pub mod feed;
pub mod generate;
pub mod lyrics;
pub mod metadata;
pub mod persona;
pub mod playlist;
pub mod remaster;
pub mod speed;
pub mod stems;
pub mod types;
pub mod upload;

mod auth_retry;
mod client;
mod headers;
mod response;

#[cfg(test)]
mod endpoint_tests;

pub use client::SunoClient;
