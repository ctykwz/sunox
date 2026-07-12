#[derive(clap::Args)]
pub struct DoctorArgs {
    /// Diagnose DNS, direct TCP, and HTTPS connectivity to Suno authentication and API endpoints
    #[arg(long)]
    pub network: bool,

    /// Return a non-zero exit code when a requested diagnostic is degraded
    #[arg(long, requires = "network")]
    pub strict: bool,
}
