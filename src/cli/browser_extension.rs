#[derive(clap::Args)]
pub struct InstallBrowserExtensionArgs {
    /// Custom directory for the unpacked extension
    #[arg(long)]
    pub path: Option<String>,

    /// Replace an existing extracted extension
    #[arg(short, long)]
    pub force: bool,
}
