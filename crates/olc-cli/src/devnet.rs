//! Starts several nodes on one computer, connected to each other, each printing its own lines.
use olc_board::Args;
use std::process::{Child, Command, Stdio};

const BASE_PORT: u16 = 7100;

pub fn run(args: &Args) -> Result<(), String> {
    let nodes: u16 = args.number("nodes", 3)?;
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut children: Vec<Child> = Vec::new();
    for index in 0..nodes {
        let mut command = Command::new(&program);
        command.stdin(Stdio::null()).args([
            "node",
            "--name",
            &format!("node{index}"),
            "--data",
            &format!(".olc/devnet/node{index}"),
            "--port",
            &(BASE_PORT + index).to_string(),
            "--mine",
        ]);
        if index > 0 {
            command.args(["--connect", &format!("127.0.0.1:{}", BASE_PORT + index - 1)]);
        }
        if let Some(coin) = args.value("coin") {
            command.args(["--coin", coin]);
        }
        if let Some(limit) = args.value("hash-limit") {
            command.args(["--hash-limit", limit]);
        }
        children.push(command.spawn().map_err(|error| error.to_string())?);
    }
    for mut child in children {
        let _ = child.wait();
    }
    Ok(())
}
