#![feature(once_cell)]

use std::collections::{HashSet, HashMap};

mod config;
mod irc_client;

use tokio::fs;

use serde::Deserialize;

use tokio_stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};

async fn make_dummy_string() -> String {
    "".to_string()
}

#[derive(Deserialize, Debug)]
struct ProgramConfig {
    twitch_config: irc_client::BotConfig,
    channel_configs: HashMap<String, irc_client::InputChannelConfig>
}


#[tokio::main]
async fn main() {
    let config_json = fs::read_to_string("config.json").await.unwrap();

    let bc : ProgramConfig = serde_json::from_str(&config_json).expect("could not read config");
    
    println!("config:\n {}", config_json);

    for (channel, conf) in &bc.channel_configs {
        irc_client::prepare_channel(channel, conf).await;
    }

    let mut sigint_stream = signal(SignalKind::interrupt()).unwrap();
    let mut twitch_client = irc_client::new_twitch(bc.twitch_config).await.unwrap();

    let mut twitch_stream = twitch_client.stream()
        .map_err(|e| {
            eprintln!("cannot get twitch stream: {}", e);
        })
        .unwrap();

    'main: loop {
        let text = tokio::select! {
            irc_msg = twitch_stream.next() => {
                if irc_msg.is_some() {
                    irc_client::handle_msg(
                        &twitch_client,
                        irc_msg.unwrap().unwrap()
                    ).await
                } else {
                    make_dummy_string().await
                }
            }
            _ = sigint_stream.recv() => {
                println!("got sigint");
                break 'main;
            }
        };

        if !text.is_empty() { println!("{}", text); }
    }
}
