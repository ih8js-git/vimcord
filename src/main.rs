use std::{env, io, process, sync::Arc};

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};
use reqwest::Client;
use tokio::{
    sync::{
        Mutex,
        mpsc::{self},
    },
    task::JoinHandle,
    time::{self, Duration},
};

use crate::{
    api::{get_channel_messages, get_current_user_guilds},
    model::{Channel, Emoji, Guild, Message},
    signals::{restore_terminal, setup_ctrlc_handler},
    ui::{draw_ui, handle_input_events, handle_keys_events},
};

mod api;
mod model;
mod signals;
mod ui;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

const UNICODE_EMOJI_DICTIONARY: [(&str, &str); 10] = [
    ("smile", "😊"),
    ("laugh", "😂"),
    ("love", "❤️"),
    ("thumbsup", "👍"),
    ("thumbsdown", "👎"),
    ("star", "⭐"),
    ("fire", "🔥"),
    ("heart_eyes", "😍"),
    ("sad", "😢"),
    ("tada", "🎉"),
];

#[derive(Debug)]
pub enum KeywordAction {
    Continue,
    Break,
}

#[derive(Debug)]
enum AppState {
    SelectingGuild,
    SelectingChannel(String),
    Chatting(String),
    EmojiSelection(String),
}

#[derive(Debug)]
pub enum AppAction {
    InputChar(char),
    InputBackspace,
    InputEscape,
    InputSubmit,
    SelectNext,
    SelectPrevious,
    ApiUpdateMessages(Vec<Message>),
    ApiUpdateChannel(Vec<Channel>),
    ApiUpdateEmojis(Vec<Emoji>),
    #[allow(dead_code)]
    TransitionToChat(String),
    TransitionToChannels(String),
    #[allow(dead_code)]
    TransitionToGuilds,
    SelectEmoji,
}

pub struct App {
    state: AppState,
    guilds: Vec<Guild>,
    channels: Vec<Channel>,
    messages: Vec<Message>,
    custom_emojis: Vec<Emoji>,
    input: String,
    selection_index: usize,
    status_message: String,
    terminal_height: usize,
    terminal_width: usize,
    emoji_filter: String,
}

async fn run_app(token: String) -> Result<(), Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = Client::new();

    let app_state = Arc::new(Mutex::new(App {
        state: AppState::SelectingGuild,
        guilds: Vec::new(),
        channels: Vec::new(),
        messages: Vec::new(),
        custom_emojis: Vec::new(),
        input: String::new(),
        selection_index: 0,
        status_message: "Loading servers...".to_string(),
        terminal_height: 20,
        terminal_width: 80,
        emoji_filter: String::new(),
    }));

    let (tx_action, mut rx_action) = mpsc::channel::<AppAction>(32);
    let (tx_shutdown, _) = tokio::sync::broadcast::channel::<()>(1);

    let tx_input = tx_action.clone();
    let rx_shutdown_input = tx_shutdown.subscribe();

    let input_handle: JoinHandle<Result<(), io::Error>> = tokio::spawn(async move {
        let res = handle_input_events(tx_input, rx_shutdown_input).await;
        if let Err(e) = &res {
            eprintln!("Input Error: {e}");
        }
        res
    });

    let api_state = Arc::clone(&app_state);
    let api_client = client.clone();
    let api_token = token.clone();
    let tx_api = tx_action.clone();
    let mut rx_shutdown_api = tx_shutdown.subscribe();

    let mut interval = time::interval(Duration::from_secs(2));

    let api_handle: JoinHandle<()> = tokio::spawn(async move {
        match get_current_user_guilds(&api_client, &api_token).await {
            Ok(guilds) => {
                let mut state = api_state.lock().await;
                state.guilds = guilds;
                state.status_message =
                    "Select a server. Use arrows to navigate, Enter to select & Esc to quit."
                        .to_string();
            }
            Err(e) => {
                api_state.lock().await.status_message = format!("Failed to load servers. {e}");
            }
        }

        loop {
            tokio::select! {
                _ = rx_shutdown_api.recv() => {
                    return;
                }

                _ = interval.tick() => {
                    let current_channel_id = {
                        let state = api_state.lock().await;
                        match &state.state {
                            AppState::Chatting(id) => Some(id.clone()),
                            _ => None,
                        }
                    };

                    if let Some(channel_id) = current_channel_id {
                        const MESSAGE_LIMIT: usize = 100;

                        match get_channel_messages(
                            &api_client,
                            &channel_id,
                            &api_token,
                            None,
                            None,
                            None,
                            Some(MESSAGE_LIMIT),
                        )
                        .await
                        {
                            Ok(messages) => {
                                if let Err(e) = tx_api.send(AppAction::ApiUpdateMessages(messages)).await {
                                    eprintln!("Failed to send message update action: {e}");
                                    return;
                                }
                            }
                            Err(e) => {
                                api_state.lock().await.status_message = format!("Error loading chat: {e}");
                            }
                        }
                    }
                }
            }
        }
    });

    loop {
        {
            let mut state_guard = app_state.lock().await;
            terminal
                .draw(|f| {
                    draw_ui(f, &mut state_guard);
                })
                .unwrap();
        }
        if let Some(action) = rx_action.recv().await {
            let state = app_state.lock().await;

            let filtered_unicode: Vec<(&str, &str)> = UNICODE_EMOJI_DICTIONARY
                .iter()
                .filter(|(name, _)| name.starts_with(&state.emoji_filter))
                .copied()
                .collect();

            let filtered_custom: Vec<&Emoji> = state
                .custom_emojis
                .iter()
                .filter(|e| e.name.starts_with(&state.emoji_filter))
                .collect();

            let total_filtered_emojis = filtered_unicode.len() + filtered_custom.len();

            match handle_keys_events(
                app_state.lock().await,
                action,
                &client,
                token.clone(),
                tx_action.clone(),
                filtered_unicode,
                filtered_custom,
                total_filtered_emojis,
            )
            .await
            {
                Some(KeywordAction::Continue) => continue,
                Some(KeywordAction::Break) => break,
                None => {}
            }
        }
    }

    drop(rx_action);

    api_handle.abort();
    input_handle.abort();
    tx_shutdown.send(()).ok();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    const ENV_TOKEN: &str = "DISCORD_TOKEN";

    let token: String = env::var(ENV_TOKEN).unwrap_or_else(|_| {
        eprintln!("Env Error: DISCORD_TOKEN variable is missing.");
        process::exit(1);
    });

    setup_ctrlc_handler();

    let app_result = run_app(token).await;

    restore_terminal();

    app_result
}
