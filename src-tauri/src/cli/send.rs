use std::path::PathBuf;

use clap::Args;
use uuid::Uuid;

use crate::config::teams;
use crate::phone::types::OutboxMessage;

use super::task_resolution;

#[derive(Args)]
#[command(after_help = "\
DELIVERY MODES:\n  \
  wake            Inject into PTY if the destination agent is idle (waiting for input). Reject otherwise.\n  \
  active-only     Inject into PTY if the destination agent is actively running (not idle). Reject otherwise.\n  \
  wake-and-sleep  Spawn a temporary session for the destination agent, inject the notification, destroy when done.\n\n\
MESSAGE TRANSPORT: `--message` accepts only a GitHub issue comment URL in this format:\n  \
  https://github.com/<owner>/<repo>/issues/<number>#issuecomment-<comment_id>\n  \
Long-form message content must be posted in GitHub first. `send` only notifies the receiver about the comment URL.\n\n\
ROUTING: Before delivery, the CLI validates that the sender can reach the destination based on team \
membership and coordinator rules (teams.json). If routing fails, the CLI exits immediately with code 1.\n\n\
DISCOVERY: Use `list-peers` to get valid agent names for --to. The \"name\" field in the JSON output \
is the value to use.\n\n\
TASKS: Message delivery resolves the sender workgroup from --root and requires exactly one active \
task record under sibling repo-*/_plans/tasks/*.json.")]
pub struct SendArgs {
    /// Session token for authentication (from '# === Session Credentials ===' block)
    #[arg(long)]
    pub token: Option<String>,

    /// Destination agent name (e.g., "repos/my-project"). Use `list-peers` to discover valid names
    #[arg(long)]
    pub to: String,

    /// GitHub issue comment URL. Required unless --command is used.
    #[arg(long)]
    pub message: Option<String>,

    /// Delivery mode (see DELIVERY MODES below)
    #[arg(long, default_value = "wake")]
    pub mode: String,

    /// Remote command to execute on the agent's PTY [possible values: clear, compact].
    /// The agent must be idle. Cannot be combined with --message
    #[arg(long)]
    pub command: Option<String>,

    /// Agent CLI to use for wake-and-sleep mode
    #[arg(long, default_value = "auto")]
    pub agent: String,

    /// Agent root directory (required). Your working directory — used to derive your agent name
    #[arg(long)]
    pub root: Option<String>,

    /// Write command-only outbox traffic to a specific directory instead of <root>/<local-dir>/outbox/
    #[arg(long)]
    pub outbox: Option<String>,
}

pub fn execute(args: SendArgs) -> i32 {
    let root = match args.root {
        Some(ref r) => r.clone(),
        None => {
            eprintln!("Error: --root is required. Specify your agent's root directory.");
            return 1;
        }
    };

    let is_root = match crate::cli::validate_cli_token(&args.token) {
        Ok((_token, root)) => root,
        Err(msg) => {
            eprintln!("{}", msg);
            return 1;
        }
    };

    let sender = crate::cli::agent_name_from_root(&root);
    let ac_dir = PathBuf::from(&root).join(crate::config::agent_local_dir_name());

    let valid_modes = ["active-only", "wake", "wake-and-sleep"];
    if !valid_modes.contains(&args.mode.as_str()) {
        eprintln!(
            "Error: invalid mode '{}'. Valid: {}",
            args.mode,
            valid_modes.join(", ")
        );
        return 1;
    }

    if !is_root {
        let discovered = teams::discover_teams();
        if !teams::can_communicate(&sender, &args.to, &discovered) {
            eprintln!(
                "Error: routing rejected — '{}' cannot reach '{}'. Check team membership and coordinator rules.",
                sender, args.to
            );
            return 1;
        }
    }

    let message = args
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    match (message.is_some(), args.command.is_some()) {
        (false, false) => {
            eprintln!("Error: --message or --command is required");
            return 1;
        }
        (true, true) => {
            eprintln!("Error: --command cannot be combined with --message");
            return 1;
        }
        _ => {}
    }

    const ALLOWED_COMMANDS: &[&str] = &["clear", "compact"];
    if let Some(ref cmd) = args.command {
        if !ALLOWED_COMMANDS.contains(&cmd.as_str()) {
            eprintln!(
                "Error: unsupported command '{}'. Allowed: {}",
                cmd,
                ALLOWED_COMMANDS.join(", ")
            );
            return 1;
        }
    }

    let task_context = if let Some(ref comment_url) = message {
        if args.outbox.is_some() {
            eprintln!(
                "Error: --outbox cannot be used with --message during the GitHub comment URL migration"
            );
            return 1;
        }

        match task_resolution::resolve_message_context(&root, comment_url) {
            Ok(context) => Some(context),
            Err(err) => {
                eprintln!("Error: {}", err);
                return 1;
            }
        }
    } else {
        None
    };

    let msg_id = Uuid::new_v4().to_string();
    let message = OutboxMessage {
        id: msg_id.clone(),
        token: args.token,
        from: sender.clone(),
        to: args.to.clone(),
        comment_url: task_context
            .as_ref()
            .map(|ctx| ctx.comment.html_url.clone()),
        legacy_body: None,
        mode: args.mode,
        legacy_get_output: false,
        request_id: None,
        sender_agent: None,
        preferred_agent: args.agent,
        priority: "normal".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        command: args.command,
        action: None,
        target: None,
        force: None,
        timeout_secs: None,
        task_id: task_context.as_ref().map(|ctx| ctx.task.id.clone()),
        task_summary: task_context.as_ref().map(|ctx| ctx.task.summary.clone()),
        github_owner: task_context
            .as_ref()
            .map(|ctx| ctx.task.github.owner.clone()),
        github_repo: task_context
            .as_ref()
            .map(|ctx| ctx.task.github.repo.clone()),
        github_issue_number: task_context
            .as_ref()
            .map(|ctx| ctx.task.github.issue_number),
        messaging_mode: task_context
            .as_ref()
            .map(|ctx| ctx.task.messaging.mode.clone()),
    };

    let outbox_dir = if let Some(ref outbox_path) = args.outbox {
        PathBuf::from(outbox_path)
    } else if is_root {
        let app_outbox = crate::config::config_dir()
            .map(|d| d.join("app-outbox-path.txt"))
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .map(|s| PathBuf::from(s.trim()));
        match app_outbox {
            Some(p) if p.is_dir() => p,
            _ => ac_dir.join("outbox"),
        }
    } else {
        ac_dir.join("outbox")
    };
    if let Err(e) = std::fs::create_dir_all(&outbox_dir) {
        eprintln!("Error: failed to create outbox directory: {}", e);
        return 1;
    }

    let outbox_path = outbox_dir.join(format!("{}.json", msg_id));
    let json = match serde_json::to_string_pretty(&message) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Error: failed to serialize message: {}", e);
            return 1;
        }
    };

    if let Err(e) = std::fs::write(&outbox_path, json) {
        eprintln!("Error: failed to write outbox file: {}", e);
        return 1;
    }

    let delivered_path = outbox_dir
        .join("delivered")
        .join(format!("{}.json", msg_id));
    let rejected_reason_path = outbox_dir
        .join("rejected")
        .join(format!("{}.reason.txt", msg_id));

    let confirm_timeout = std::time::Duration::from_secs(30);
    let confirm_poll = std::time::Duration::from_millis(250);
    let start = std::time::Instant::now();

    loop {
        if delivered_path.exists() {
            println!("Message delivered: {}", msg_id);
            return 0;
        }
        if rejected_reason_path.exists() {
            let reason = std::fs::read_to_string(&rejected_reason_path)
                .unwrap_or_else(|_| "unknown reason".to_string());
            eprintln!("Error: message rejected — {}", reason.trim());
            return 1;
        }
        if start.elapsed() >= confirm_timeout {
            eprintln!(
                "Error: delivery confirmation timeout after 30s (message {} may still be pending)",
                msg_id
            );
            return 1;
        }
        std::thread::sleep(confirm_poll);
    }
}
