use clap::Args;

use crate::server::ServerClient;

#[derive(Args)]
pub struct StopCommand;

impl StopCommand {
    /// Stops the background process and frees the memory its models use; the next search starts it again.
    pub fn run(self) -> anyhow::Result<()> {
        match ServerClient::any() {
            Some(client) => {
                client.stop()?;
                eprintln!("Stopped the s1grep background process (pid {}).", client.info.pid);
            }
            None => eprintln!("No s1grep background process is running."),
        }
        Ok(())
    }
}
