use std::process::Command;

fn shells(user: &str) {
    // ruleid: navree-shell-command-string, navree-process-outside-tooling
    let _ = Command::new("sh").arg("-c").arg(format!("ls {user}")).output();
    // ruleid: navree-shell-command-string, navree-process-outside-tooling
    let _ = std::process::Command::new("bash").args(["-c", user]).output();
    // ruleid: navree-shell-command-string, navree-process-outside-tooling
    let _ = Command::new("cmd").args(["/C", user]).output();
    // ruleid: navree-shell-command-string, navree-process-outside-tooling
    let _ = tokio::process::Command::new("powershell").arg("-Command").arg(user).output();
}

fn direct(user: &str) {
    // ok: navree-shell-command-string
    // ruleid: navree-process-outside-tooling
    let _ = Command::new("git").arg("log").arg(user).output();
}

// The second rule reports every spawn outside the exempt crates; this fixture
// lives outside them.
fn spawns() {
    // ruleid: navree-process-outside-tooling
    let _ = Command::new("ffmpeg").output();
    // ruleid: navree-process-outside-tooling
    let _ = std::process::Command::new("git").output();
    // ruleid: navree-process-outside-tooling
    let _ = tokio::process::Command::new("git").output();
    // ruleid: navree-process-outside-tooling, navree-shell-command-string
    let _ = Command::new("sh").output();
}
