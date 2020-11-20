use std::{path::PathBuf, time::Duration};

use snafu::ResultExt;
use structopt::StructOpt;
use tokio::runtime::Runtime;
use tokio_compat_02::FutureExt;

use istinit::istio;

use crate::error::{self, Error};

#[derive(Debug, StructOpt)]
pub struct Command {
    #[structopt(long = "enable-process-subreaper", short = "s", env = "ENABLE_PROCESS_SUBREAPER")]
    enable_process_subreaper: bool,

    #[structopt(long = "with-istio", env = "WITH_ISTIO")]
    with_istio: bool,

    #[structopt(
        long = "pilot-agent-endpoint",
        env = "PILOT_AGENT_ENDPOINT",
        default_value = "http://127.0.0.1:15021"
    )]
    pilot_agent_endpoint: String,

    #[structopt(long = "kill-istio", env = "KILL_ISTIO")]
    kill_istio: bool,

    command: String,

    args: Vec<String>,
}

impl Command {
    #[inline]
    pub fn new() -> Command { Command::from_args() }

    pub fn run(self) -> Result<(), Error> {
        {
            use tracing_subscriber::prelude::*;

            let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);
            let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
                .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))
                .unwrap();

            tracing_subscriber::registry().with(filter_layer).with(fmt_layer).init();
        }

        let runtime = Runtime::new().context(error::InitializeTokioRuntime)?;
        runtime.block_on(
            async {
                if self.with_istio {
                    tracing::info!("Wait for Envoy ready");
                    let retry_interval = Duration::from_secs(3);
                    istio::wait_for_envoy_ready(&self.pilot_agent_endpoint, retry_interval, None)
                        .await
                        .context(error::WaitForEnvoyReady)?;
                }

                tracing::info!("Spawn process {} and wait", self.command);
                if let Err(err) = spawn_and_wait_executable(&self.command, &self.args).await {
                    tracing::warn!("Error: {}", err);
                };

                if self.with_istio && self.kill_istio {
                    tracing::info!("Kill Istio");
                    istio::kill_istio_with_api(&self.pilot_agent_endpoint)
                        .await
                        .context(error::KillIstio)?;
                }

                Ok(())
            }
            .compat(),
        )?;

        Ok(())
    }
}

async fn spawn_and_wait_executable(command: &str, args: &[String]) -> Result<i32, Error> {
    let mut child = tokio::process::Command::new(command)
        .args(args)
        .spawn()
        .context(error::SpawnProcess { executable_path: PathBuf::from(command) })?;

    let _status = child.wait().await.context(error::WaitForChildProcess)?;

    Ok(0)
}
