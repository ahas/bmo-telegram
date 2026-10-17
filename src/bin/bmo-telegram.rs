//! The service's binary (docs/boundary-api.md): `serve` answers the provider's routes and reads the bot
//! account's rooms until the unit tells it to stop, and `health` ends 0 only where the served service answers
//! ready, which a deployment waits on. Both read the environment the deployment's env file supplies.

use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use bmo_chat::{ChatProvider, ChatReceiver, ChatReceiverLink};
use bmo_telegram::config::Config;
use bmo_telegram::provider::TelegramProvider;
use bmo_telegram::reader::ReaderClient;
use bmo_telegram::receiver::ReceiverNeeds;
use bmo_telegram::routes::{keep_reader_current, service};
use bmo_telegram::seam::{Seam, Standing};
use bmo_telegram::seen::Seen;
use bmo_telegram::send::Sending;
use bmo_telegram::telegram::Telegram;
use bmo_telegram::wire::Wire;

#[derive(Parser)]
#[command(
  name = "bmo-telegram",
  version,
  about = "The Telegram chat provider: the bot account, its sends and the owner's reader behind it."
)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// The provider's routes answered and the bot account reading its rooms, until the unit tells it to stop.
  Serve,
  /// Whether the served service answers ready: exit code 0 where it does, 1 where it does not.
  Health,
}

// The stop the served routes answer to: the signal the unit sends, either of them once.
async fn stopped() {
  let interrupted = tokio::signal::ctrl_c();
  let terminated = async {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
      Ok(mut terminate) => {
        terminate.recv().await;
      }
      Err(_) => std::future::pending::<()>().await,
    }
  };

  tokio::select! {
    _ = interrupted => {}
    () = terminated => {}
  }
}

/// The service stood up from the environment: the provider reading its rooms, the seam the reader calls and
/// the routes served on the loopback, until a stop takes the reading down and ends the process.
async fn serve() -> Result<(), String> {
  let config = Config::from_environment();
  let missing = config.missing();

  if !missing.is_empty() {
    for name in missing {
      eprintln!("telegram: {name} is not set");
    }

    return Err("the configuration is incomplete".to_owned());
  }

  if let Err(why) = std::fs::create_dir_all(&config.data) {
    return Err(format!("the data folder {} could not be made: {why}", config.data.display()));
  }

  let seen = Arc::new(Seen::kept_in(config.data.join("seen.json")));
  let standing = Arc::new(Standing::default());
  let reader = ReaderClient::new(Wire::loopback(config.reader_port, config.token.as_str()));
  let telegram = Arc::new(Telegram::new(
    &config.bot_token,
    &config.api,
    config.owner_id,
    Some(reader.clone()),
    seen.clone(),
  ));
  let link = ChatReceiverLink::new(config.receiver_url.as_str(), config.receiver_token.as_str());
  let receiver: Arc<dyn ChatReceiver> = Arc::new(link);
  let seam = Arc::new(Seam {
    token: config.token,
    receiver: receiver.clone(),
    needs: ReceiverNeeds::new(Wire::new(config.receiver_url.as_str(), config.receiver_token.as_str())),
    seen: seen.clone(),
    standing: standing.clone(),
  });
  let provider =
    Arc::new(TelegramProvider::new(telegram, Sending::default(), Some(reader), seen, standing));

  provider.start(receiver).await.map_err(|why| why.to_string())?;

  let listener = tokio::net::TcpListener::bind(("127.0.0.1", config.port))
    .await
    .map_err(|why| why.to_string())?;
  let app = service(provider.clone(), seam, config.data.join("avatars"));
  let on = config.port;

  println!("telegram: serving on 127.0.0.1:{on}");

  let refreshing = keep_reader_current(provider.clone(), std::time::Duration::from_secs(5));
  let served = axum::serve(listener, app)
    .with_graceful_shutdown(stopped())
    .await
    .map_err(|why| why.to_string());

  provider.stop().await;
  refreshing.abort();

  served
}

/// The served service asked whether it is ready, for a deployment to wait on.
async fn health() -> Result<(), String> {
  let config = Config::from_environment();

  if config.token.is_empty() {
    return Err("BMO_TELEGRAM_TOKEN is not set".to_owned());
  }

  let answer = Wire::loopback(config.port, config.token).get("/health").await;

  match answer {
    Ok(answer) if answer.body.get("ready").and_then(|ready| ready.as_bool()) == Some(true) => Ok(()),
    Ok(answer) => {
      let status = answer.status;
      let reason = answer.body.get("reason").and_then(|why| why.as_str()).unwrap_or("the service is down");

      Err(format!("the service answered {status}: {reason}"))
    }
    Err(why) => Err(why),
  }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let cli = Cli::parse();

  let ended = match cli.command {
    Command::Serve => serve().await.map(|()| 0),
    Command::Health => health().await.map(|()| 0),
  };

  match ended {
    Ok(code) => ExitCode::from(code),
    Err(why) => {
      eprintln!("bmo-telegram: {why}");

      ExitCode::from(1)
    }
  }
}
