//! Starts a node process under limits the operating system enforces.
use super::{NodeSpec, PROCESS_OVERHEAD_MIB};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

const MIB: u64 = 1024 * 1024;

/// What holds a node to its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enforcement {
    /// A systemd scope: memory and CPU are both held by the kernel.
    Systemd,
    /// An address-space limit. CPU share is not held, and only the hash cap slows the node.
    Rlimit,
    Nothing,
}

fn works(program: &str, words: &[&str]) -> bool {
    Command::new(program)
        .args(words)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn enforcement() -> Enforcement {
    static FOUND: OnceLock<Enforcement> = OnceLock::new();
    *FOUND.get_or_init(|| {
        if works("systemd-run", &["--user", "--scope", "-q", "true"]) {
            Enforcement::Systemd
        } else if works("prlimit", &["--as=1000000000", "true"]) {
            Enforcement::Rlimit
        } else {
            Enforcement::Nothing
        }
    })
}

/// The whole process's memory: what it was given for chain data, and what running takes.
pub fn memory_limit_bytes(spec: &NodeSpec) -> u64 {
    (spec.memory_mib + PROCESS_OVERHEAD_MIB) * MIB
}

/// The program and words to run `node` under `spec`'s limits, by `how`.
pub fn wrapped(how: Enforcement, spec: &NodeSpec, program: &str, node: &[String]) -> Command {
    let memory = memory_limit_bytes(spec);
    match how {
        Enforcement::Systemd => {
            let mut command = Command::new("systemd-run");
            command.args(["--user", "--scope", "-q", "--collect"]);
            command.arg(format!("--unit=olc-node-{}-{}", spec.id, std::process::id()));
            command.arg("-p").arg(format!("MemoryMax={memory}"));
            command.args(["-p", "MemorySwapMax=0"]);
            if spec.cpu_percent > 0 {
                command.arg("-p").arg(format!("CPUQuota={}%", spec.cpu_percent));
            }
            command.arg(program).args(node);
            command
        }
        Enforcement::Rlimit => {
            let mut command = Command::new("prlimit");
            // Address space counts reserved stacks and mapped code as well, so it gets more room
            // than resident memory would.
            command.arg(format!("--as={}", memory * 8));
            command.arg("--").arg(program).args(node);
            command
        }
        Enforcement::Nothing => {
            let mut command = Command::new(program);
            command.args(node);
            command
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Preset;
    use super::*;

    fn words(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|word| word.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn an_esp32_node_gets_its_memory_held_by_the_kernel() {
        let spec = Preset::Esp32S3.spec(3);
        let command = wrapped(Enforcement::Systemd, &spec, "olc-core", &["node".into()]);
        let args = words(&command);
        let limit = format!("MemoryMax={}", (8 + PROCESS_OVERHEAD_MIB) * MIB);
        assert!(args.contains(&limit));
        assert!(!args.iter().any(|word| word.starts_with("CPUQuota")));
        assert_eq!(args.last().map(String::as_str), Some("node"));
    }

    #[test]
    fn a_cpu_share_becomes_a_quota() {
        let spec = Preset::RaspberryPi4.spec(1);
        let command = wrapped(Enforcement::Systemd, &spec, "olc-core", &[]);
        assert!(words(&command).contains(&"CPUQuota=400%".to_string()));
    }

    #[test]
    fn without_systemd_an_address_limit_stands_in() {
        let spec = Preset::Esp32S3.spec(1);
        let command = wrapped(Enforcement::Rlimit, &spec, "olc-core", &["node".into()]);
        assert_eq!(command.get_program(), "prlimit");
    }
}
