use std::collections::{HashSet, HashMap};
use std::lazy::SyncOnceCell;
use tokio::sync::RwLock;

use serde::Deserialize;

use irc::client::prelude::Message;
use itertools::Itertools;

use linkify::{LinkFinder, LinkKind};

const SHORT_TIMEOUT_SECONDS: u32 = 10;
static GLOBAL_BLACKLIST: SyncOnceCell<HashSet<String>> = SyncOnceCell::new();

fn global_blacklist() -> &'static HashSet<String> {
    GLOBAL_BLACKLIST.get_or_init(|| {
        let mut blacklist = HashSet::new();

        blacklist.insert("reload-conf".to_owned());
        blacklist.insert("update".to_owned());
        blacklist.insert("edit".to_owned());
        blacklist.insert("add".to_owned());
        blacklist.insert("newmod".to_owned());

        blacklist
    })
}

#[derive(PartialEq, Debug)]
pub enum Role {
    Owner,
    Mode,
    Peasant,
}

#[derive(Deserialize, Debug)]
pub struct InputChannelConfig {
    pub known_users : Option<HashSet<String>>,
    pub bot_admins : Option<HashSet<String>>,
    pub channel_text_commands : Option<HashMap<String, String>>,
    pub command_blacklist : Option<HashSet<String>>,
}

struct ChannelConfig {
    known_users : HashSet<String>,
    bot_admins : HashSet<String>,
    channel_text_commands : HashMap<String, String>,
    command_blacklist : HashSet<String>,
}

fn global_state_map() -> &'static RwLock<HashMap<String, ChannelConfig>> {
    static CLIENT_STATE: SyncOnceCell<RwLock<HashMap<String, ChannelConfig>>> = SyncOnceCell::new();
    CLIENT_STATE.get_or_init(|| {
        RwLock::new(HashMap::new())
    })
}

fn load_channel_state(channel: &str) -> ChannelConfig {
    // read from json file using serde, or return default if file
    // doesn't exist
    return ChannelConfig {
        known_users: HashSet::new(),
        bot_admins : HashSet::new(),
        channel_text_commands : HashMap::new(),
        command_blacklist : global_blacklist().to_owned(),
    }
}

pub async fn prepare_channel(channel: &str, conf: &InputChannelConfig) {
    {
        if global_state_map().read().await.contains_key(channel) {
            eprintln!("cannot configure channel {}, already configured", channel);
            return
        }
    }

    let mut unlocked_state = global_state_map().write().await;
    unlocked_state.insert(channel.to_owned(), ChannelConfig {
        known_users: match &conf.known_users {
            Some(users) => users.to_owned(),
            None => HashSet::new()
        },
        bot_admins: match &conf.bot_admins {
            Some(users) => users.to_owned(),
            None => HashSet::new()
        },
        channel_text_commands: match &conf.channel_text_commands {
            Some(cmds) => cmds.to_owned(),
            None => HashMap::new()
        },
        command_blacklist: match &conf.command_blacklist {
            Some(blacklist) => {
                let mut combined : HashSet<String> = HashSet::new();

                for cmd in global_blacklist().union(blacklist) {
                    combined.insert(cmd.to_owned());
                }

                combined
            }
            None => global_blacklist().to_owned()
        }
    });
}

fn save_state() {
}

#[derive(Deserialize, Debug)]
pub struct BotConfig {
    pub nickname: String,
    pub server: String,
    pub port: u16,
    pub channels: Vec<String>,
    pub use_tls: Option<bool>,
    pub auth_token: Option<String>
}

pub async fn new_twitch(bot_config: BotConfig) -> Result<irc::client::Client, anyhow::Error> {
    let irc_conf = irc::client::prelude::Config {
        nickname: Some(bot_config.nickname.to_owned()),
        server: Some(bot_config.server.to_owned()),
        port: Some(bot_config.port),
        channels: bot_config.channels,
        use_tls: bot_config.use_tls,
        ..irc::client::prelude::Config::default()
    };

    let client = irc::client::Client::from_config(irc_conf).await?;
    client.send(format!("PASS {}", bot_config.auth_token.unwrap()).as_str()).unwrap();
    client.identify()?;

    Ok(client)
}

struct ParsedMessage {
    sender: String,
    channel: String,
    command: String,
    body: String
}

fn parse_irc_command(command: &str, sender: &str, raw: &str) -> ParsedMessage
{
    let (cmd, channel, msg) = match command {
        "PRIVMSG" => {
            let actual_start = raw.find("PRIVMSG").unwrap();
            let triple = unsafe { raw.get_unchecked(actual_start..) };
            triple.splitn(3, ' ')
                .collect_tuple()
                .unwrap_or_else(|| {
                    eprintln!("Couldn't split {} to command, channel and message", raw);
                    ("unknown", "unknown", "")
                })
        }
        _ => {
            ("unknown", "unknown", "")
        }
    };

    ParsedMessage {
        sender: sender.to_string(),
        channel: channel.to_string(),
        command: cmd.to_string(),
        body: msg.trim_start_matches(':').to_string(),
    }
}

fn get_parsed_message(input: &Message) -> Option<ParsedMessage> {
    let recognized_commands = ["PRIVMSG"];

    let sender = input.source_nickname();
    let raw_str = input.to_string();
    let raw_str = raw_str.trim();

    for command in &recognized_commands {
        if raw_str.contains(command) {
            return Some(parse_irc_command(command, sender.unwrap_or("unknown"), raw_str))
        }
    }
    None
}

async fn get_role_from(input: &ParsedMessage) -> Role {
    if input.sender == input.channel.trim_start_matches("#") {
        Role::Owner
    } else {
        let read_state = global_state_map().read().await;
        if read_state.get(&input.channel).unwrap().bot_admins.contains(&input.sender) {
            Role::Mode
        } else {
            Role::Peasant
        }
    }
}

async fn is_known_user(input: &ParsedMessage) -> bool {
    let state = global_state_map();
    {
        let unlocked_state = state.read().await;
        if !unlocked_state.contains_key(&input.channel) {
            panic!("access invalid channel");
        }

        if unlocked_state.get(&input.channel).unwrap().known_users.contains(&input.sender) {
            true
        } else {
            false
        }
    }
}

async fn ensure_channel_data_is_loaded(input: &ParsedMessage) {
    let state = global_state_map();
    {
        let read_state = state.read().await;
        if read_state.contains_key(&input.channel) {
            return;
        }
    }

    eprintln!("channel data not loaded yet");
    let mut unlocked_state = state.write().await;
    unlocked_state.insert(input.channel.to_string(), load_channel_state(&input.channel));
}

fn purge_links(client: &irc::client::Client, msg: &ParsedMessage) -> bool {
    let mut link_finder = LinkFinder::new();
    link_finder.kinds(&[LinkKind::Url]);

    let links: Vec<_> = link_finder.links(&msg.body).collect();

    if links.len() > 0 {
        eprintln!("purge {} for links", &msg.sender);
        let cmd = format!("/timeout {} {} ei linkkejä", msg.sender, SHORT_TIMEOUT_SECONDS);
        client.send_privmsg(&msg.channel, cmd).unwrap();
        true
    } else {
        false
    }
}

fn purge_zalgo(client: &irc::client::Client, message: &ParsedMessage) -> bool {
    // 0x0300 - 0x036f is unicode combining diacritics block
    for c in message.body.chars() {
        if 0x0300 <= (c as u32) && (c as u32) <= 0x036f {
            eprintln!("purge {} for zalgo", &message.sender);
            let cmd = format!("/timeout {} {} ei zalgoa", message.sender, SHORT_TIMEOUT_SECONDS);
            client.send_privmsg(&message.channel, cmd).unwrap();
            return true
        }
    }

    false
}

async fn mark_user_as_known(user: &str, channel: &str) {
    let mut unlocked_state = global_state_map().write().await;

    unlocked_state.get_mut(channel).unwrap()
        .known_users.insert(user.to_owned());
}

fn moderate_message(client: &irc::client::Client, message: &ParsedMessage, _role: &Role, known: &bool) -> bool {
    let purged_known_user = if !known { 
        purge_links(&client, &message)
    } else {
        false
    };

    purged_known_user || 
    purge_zalgo(&client, &message)
}   

async fn handle_bot_command(client: &irc::client::Client, message: &ParsedMessage, role: &Role, _known: &bool) -> bool {
    if message.body.starts_with('!') {
        let parts: Vec<&str> = message.body.splitn(2, ' ').collect();
        eprintln!("command with {} parts", parts.len());
        {
            let read_state = global_state_map().read().await;
            let blacklist = &read_state.get(&message.channel).unwrap().command_blacklist;

            println!("{:?}", blacklist);

            if blacklist.contains(parts[0]) || blacklist.contains(parts[0].trim_start_matches("!")) {
                eprintln!("blacklisted command {} ignored", parts[0]);
                return false;
            }
        }
        match parts.len() {
            1 => { 
                let read_state = global_state_map().read().await;
                match read_state.get(&message.channel).unwrap().channel_text_commands.get(parts[0]) {
                    Some(reply) => { 
                        client.send_privmsg(&message.channel, reply).unwrap();
                        true
                    }
                    None => false
                }
            },
            2 => { if role != &Role::Peasant {
                let mut unlocked_state = global_state_map().write().await;
                unlocked_state.get_mut(&message.channel).unwrap()
                    .channel_text_commands.insert(parts[0].to_string(), parts[1].to_owned());
                true
            } else {
                false
            }},
            _ => { false }
        }
    } else {
        false
    }
}

pub async fn handle_msg(client: &irc::client::Client, raw: Message) -> String {
    let message = get_parsed_message(&raw);

    if message.is_none() {
        println!("unparsed: {}", raw.to_string().trim());
        return String::new()
    }

    let message = message.unwrap();

    ensure_channel_data_is_loaded(&message).await;

    let role = get_role_from(&message).await;
    let known_user = is_known_user(&message).await;

    if !moderate_message(&client, &message, &role, &known_user) {
        if !known_user { mark_user_as_known(&message.sender, &message.channel).await; }
    }

    handle_bot_command(&client, &message, &role, &known_user).await;

    eprintln!("msg from {:?} {}", &role, &message.sender);
/*
    if role == Role::Owner {
        client.send_privmsg(message.channel, "/mods").unwrap();
    }
*/
    message.body
}
