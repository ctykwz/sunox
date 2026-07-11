#[derive(clap::Args)]
pub struct DoctorArgs {
    /// Diagnose DNS, direct TCP, and HTTPS connectivity to Suno authentication and API endpoints
    #[arg(long)]
    pub network: bool,
}
