//! Authentication domain: local auth state, browser cookies, Clerk exchange, and headers.

mod browser;
mod clerk;
mod cookie;
mod environment;
mod interactive;
mod state;
mod token;
mod types;

pub use browser::extract_browser_auth;
pub use clerk::{clerk_refresh_jwt, clerk_token_exchange};
pub use cookie::normalize_cookie_input;
pub(crate) use cookie::{is_suno_auth_cookie_domain, is_suno_cookie_domain};
pub use interactive::{delete_interactive_browser_profile, extract_interactive_browser_auth};
pub use state::AuthState;
pub use token::browser_token;
#[allow(unused_imports)]
pub use types::{BrowserAuth, BrowserEnvironment};
