//! The interactive multi-turn REPL.
//!
//! `agent` with no goal enters this loop: read a line, run it as a turn (the
//! working set persists across turns), and print the answer. Lines beginning with
//! `/` are slash commands. Ctrl-D (EOF) or `/quit` exits. Each turn's transcript
//! is saved under `.agent/sessions/` so it can be resumed.

use agent_core::Message;
use agent_runtime::{session_store, Agent, Session};
use std::io::{BufRead, Write};
use std::path::Path;

/// Run the REPL until EOF or `/quit`. `initial` optionally seeds the session with
/// a resumed transcript (its id + messages).
pub async fn run(
    agent: &Agent,
    sessions_dir: &Path,
    initial: Option<(String, Vec<Message>)>,
) -> anyhow::Result<()> {
    let mut session = agent.session();
    let mut id = match initial {
        Some((rid, msgs)) => {
            session.load(msgs);
            println!("resumed session {rid}");
            rid
        }
        None => new_id(),
    };

    println!("agent-seddon REPL — type a goal, or /help for commands. Ctrl-D to exit.");

    let stdin = std::io::stdin();
    loop {
        print!("\n> ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            println!(); // newline after ^D
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(cmd) = line.strip_prefix('/') {
            if handle_command(cmd, agent, &mut session, &mut id, sessions_dir).await == Flow::Quit {
                break;
            }
            continue;
        }

        match session.send(line).await {
            Ok(answer) => {
                println!("{answer}");
                if let Err(e) = session_store::save(sessions_dir, &id, session.messages()) {
                    tracing::warn!("could not save session: {e}");
                }
            }
            Err(e) => eprintln!("error: {e}"),
        }
    }
    Ok(())
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Quit,
}

async fn handle_command<'a>(
    cmd: &str,
    agent: &'a Agent,
    session: &mut Session<'a>,
    id: &mut String,
    sessions_dir: &Path,
) -> Flow {
    let mut parts = cmd.split_whitespace();
    match parts.next().unwrap_or("") {
        "quit" | "exit" | "q" => return Flow::Quit,
        "help" | "h" => {
            println!(
                "commands:\n  \
                 /help            this help\n  \
                 /new             start a fresh session\n  \
                 /compact         compact the context now\n  \
                 /resume          pick a saved session to resume\n  \
                 /model           show the model\n  \
                 /tools           list available tools\n  \
                 /save            save the current session\n  \
                 /quit            exit"
            );
        }
        "new" => {
            *session = agent.session();
            *id = new_id();
            println!("started a new session");
        }
        "compact" => match session.compact().await {
            Ok(()) => println!("(context compacted)"),
            Err(e) => eprintln!("compact failed: {e}"),
        },
        "model" => println!("model: {}", agent.model()),
        "tools" => println!("tools: {}", agent.tool_names().join(", ")),
        "save" => match session_store::save(sessions_dir, id, session.messages()) {
            Ok(()) => println!("saved session {id}"),
            Err(e) => eprintln!("save failed: {e}"),
        },
        "resume" => resume_picker(agent, session, id, sessions_dir),
        other => println!("unknown command /{other} (try /help)"),
    }
    Flow::Continue
}

/// List saved sessions and load the one the user picks by index.
fn resume_picker<'a>(
    agent: &'a Agent,
    session: &mut Session<'a>,
    id: &mut String,
    sessions_dir: &Path,
) {
    let infos = session_store::list(sessions_dir);
    if infos.is_empty() {
        println!("no saved sessions");
        return;
    }
    for (i, s) in infos.iter().enumerate() {
        println!("  [{i}] {} turn(s)  {}", s.turns, s.preview);
    }
    print!("resume which? [index, or blank to cancel]: ");
    std::io::stdout().flush().ok();

    let mut choice = String::new();
    if std::io::stdin().lock().read_line(&mut choice).is_err() {
        return;
    }
    let choice = choice.trim();
    if choice.is_empty() {
        return;
    }
    let Ok(idx) = choice.parse::<usize>() else {
        println!("not a number");
        return;
    };
    let Some(info) = infos.get(idx) else {
        println!("out of range");
        return;
    };
    match session_store::load(sessions_dir, &info.id) {
        Ok(msgs) => {
            *session = agent.session();
            session.load(msgs);
            *id = info.id.clone();
            println!("resumed session {} ({} turns)", info.id, info.turns);
        }
        Err(e) => eprintln!("could not load session: {e}"),
    }
}

/// A fresh session id: unix-seconds prefix (sortable) + a random suffix.
pub(crate) fn new_id() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}-{}", uuid::Uuid::new_v4().simple())
}
