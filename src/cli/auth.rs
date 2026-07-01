#[derive(clap::Args)]
pub struct AuthArgs {
    /// Auto-auth from browser cookies, falling back to an interactive Chrome/Edge login
    #[arg(long)]
    pub login: bool,

    /// Force-refresh the JWT via the stored Clerk session cookie. Use this
    /// when the CLI returns `auth_expired` or `Token validation failed`
    /// without requiring a full re-login from the browser.
    #[arg(long)]
    pub refresh: bool,

    /// JWT token (manual fallback)
    #[arg(long)]
    pub jwt: Option<String>,

    /// Clerk __client cookie (manual fallback for headless servers)
    ///
    /// Accepts either the raw __client value or a full browser Cookie header.
    #[arg(long)]
    pub cookie: Option<String>,

    /// Device ID
    #[arg(long)]
    pub device: Option<String>,

    /// Remove stored authentication and the interactive login profile
    #[arg(long)]
    pub logout: bool,
}
