//! Input for a Claude that runs in an nvim :terminal. tmux send-keys would reach nvim itself, so the
//! text goes to the terminal's job channel over nvim's RPC socket instead: it reaches that Claude
//! whatever nvim shows and whichever mode it is in.

use std::process::Command;

/// nvim's RPC socket, when pid runs inside one of its terminals (nvim sets $NVIM for its jobs).
pub fn socket_of(pid: u32) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let sock = environ.split(|&b| b == 0).find_map(|kv| kv.strip_prefix(b"NVIM="))?;
    let sock = String::from_utf8_lossy(sock).into_owned();
    std::path::Path::new(&sock).exists().then_some(sock)
}

/// Finds the terminal buffer whose job is pid or one of its parents, and sends it the bytes in _A[2] (hex).
const SEND: &str = "(function(a) \
  local bytes = a[2]:gsub('..', function(h) return string.char(tonumber(h, 16)) end) \
  for _, b in ipairs(vim.api.nvim_list_bufs()) do \
    if vim.bo[b].buftype == 'terminal' and vim.tbl_contains(a[1], vim.b[b].terminal_job_pid) then \
      return vim.fn.chansend(vim.bo[b].channel, bytes) > 0 and 'ok' or 'closed' \
    end \
  end \
  return 'none' end)(_A)";

/// Write bytes to the input of the nvim terminal that Claude (pid) runs in.
pub fn send(socket: &str, pid: u32, bytes: &str) -> Result<(), String> {
    // Claude, its shell and so on up: the terminal's job is the first of them nvim started.
    let mut pids = vec![pid];
    while let Some(p) = crate::state::parent(*pids.last().unwrap()).filter(|_| pids.len() < 8) {
        pids.push(p);
    }
    let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let hex: String = bytes.bytes().map(|b| format!("{b:02x}")).collect();
    let expr = format!("luaeval(\"{SEND}\", [[{list}], '{hex}'])");
    let out = Command::new("nvim").args(["--server", socket, "--remote-expr", &expr]).output().map_err(|e| format!("Cannot run nvim: {e}"))?;
    match String::from_utf8_lossy(&out.stdout).trim() {
        "ok" => Ok(()),
        "none" => Err("Its nvim no longer has the terminal Claude runs in.".into()),
        "closed" => Err("Its nvim terminal does not take input.".into()),
        _ => Err(format!("nvim did not answer: {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A headless nvim with a terminal running `cat > out`: what send writes must end up in out.
    #[test]
    fn send_reaches_the_terminal_job() {
        if Command::new("nvim").arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("claude-hub-nvim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (sock, out) = (dir.join("sock"), dir.join("out"));
        let sock = sock.to_str().unwrap();
        let cmd = format!("terminal sh -c 'stty raw -echo; exec cat > {}'", out.display());
        let mut nvim = Command::new("nvim").args(["--clean", "--headless", "--listen", sock, "-c", &cmd]).spawn().unwrap();
        let ask = |expr: &str| String::from_utf8_lossy(&Command::new("nvim").args(["--server", sock, "--remote-expr", expr]).output().unwrap().stdout).trim().to_string();
        let mut job = String::new();
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            job = ask("get(b:, 'terminal_job_pid', '')");
            if !job.is_empty() {
                break;
            }
        }
        let job: u32 = job.parse().expect("terminal started");
        let result = send(sock, job, "it's \"quoted\"\r");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let got = std::fs::read_to_string(&out).unwrap_or_default();
        let none = send(sock, 1, "x");
        let _ = nvim.kill();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(result, Ok(()));
        assert_eq!(got, "it's \"quoted\"\r");
        assert!(none.unwrap_err().contains("no longer has the terminal"));
    }
}
