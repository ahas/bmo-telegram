//! The messages tool's executable: one call handed by the tool host on standard input, answered on standard
//! output (the tool host's protocol 1). A call outside the owner's word, an unresolved chat or a provider that
//! answers nothing is a refusal the model reads.

use std::io::Read;
use std::process::ExitCode;

use serde_json::Value;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let mut asked = String::new();

  if let Err(why) = std::io::stdin().read_to_string(&mut asked) {
    eprintln!("ERROR: the call could not be read: {why}");

    return ExitCode::FAILURE;
  }

  let handed: Value = match serde_json::from_str(&asked) {
    Ok(handed) => handed,
    Err(why) => {
      eprintln!("ERROR: the call does not read as JSON: {why}");

      return ExitCode::FAILURE;
    }
  };

  match messages_tool::answer(&handed).await {
    Ok(answer) => {
      println!("{}", serde_json::to_string(&answer).unwrap_or_default());

      ExitCode::SUCCESS
    }
    Err(why) => {
      eprintln!("ERROR: {why}");

      ExitCode::FAILURE
    }
  }
}
