#[derive(clap::Args)]
pub struct DoctorArgs {
    /// Diagnose DNS, direct TCP, and HTTPS connectivity to Suno authentication and API endpoints
    #[arg(long, conflicts_with = "browser_bridge")]
    pub network: bool,

    /// Return a non-zero exit code when a requested diagnostic is degraded
    #[arg(long, requires = "network")]
    pub strict: bool,

    /// Verify the installed Browser Bridge can claim an authenticated loopback probe
    #[arg(long, conflicts_with = "network")]
    pub browser_bridge: bool,
}
