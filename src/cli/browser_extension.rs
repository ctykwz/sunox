#[derive(clap::Args)]
pub struct InstallBrowserExtensionArgs {
    /// Legacy path override; custom destinations are unsupported, so omit this option
    #[arg(long)]
    pub path: Option<String>,

    /// Replace an existing extracted extension
    #[arg(short, long)]
    pub force: bool,
}
