mod mutation;
mod query;

pub use mutation::{delete, dislike, empty_trash, like, publish, purge, restore, set};
pub use query::{info, list, search, status};
