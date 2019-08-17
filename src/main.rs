use std::env;
use std::process;
use std::str::Chars;
use std::sync::Arc;

extern crate env_logger;
use log::{debug, info, warn};

extern crate regex;
use regex::Regex;

extern crate kankyo;
extern crate redis;

use serenity::client::Client;
use serenity::model::channel::Message;
use serenity::prelude::*;
use serenity::prelude::{Context, EventHandler};

struct RedisConnectionContainer;
impl TypeMapKey for RedisConnectionContainer {
    type Value = Arc<Mutex<redis::Connection>>;
}

struct RegexContainer;
impl TypeMapKey for RegexContainer {
    type Value = Arc<Mutex<Regex>>;
}

fn is_emoji(character_iter: Chars) -> bool {
    for character in character_iter {
        if !unic_emoji_char::is_emoji(character) {
            return false;
        }
    }
    true
}

fn remove_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<String>()
}

struct Handler;

impl EventHandler for Handler {
    fn message(&self, ctx: Context, message: Message) {
        if message.author.bot {
            debug!(
                "ignoring a message from '{}' since its a bot",
                message.author
            );
            return;
        }

        let content: &String = &message.content;

        if content.len() >= 60 {
            debug!("'{}' is more than 60 bytes in length", content);
            return;
        }

        let length = content.chars().count();

        if length < 3 {
            debug!("'{}' is less than three characters in length", content);
            return;
        }

        let redis_command = match &content[content.len() - 2..content.len()] {
            "++" => "INCR",
            "--" => "DECR",
            _ => {
                debug!("'{}' is not a redis command", content);
                return;
            }
        };

        let content = &content[..content.len() - 2];
        let stripped_content = remove_whitespace(content);

        let data = ctx.data.read();

        if content.is_ascii() {
            let is_valid_command_mutex = match data.get::<RegexContainer>() {
                Some(regex) => regex.clone(),
                None => panic!("Didn't find RegexContainer within the context"),
            };
            let is_valid_command = &mut *is_valid_command_mutex.lock();

            if !is_valid_command.is_match(&content) {
                debug!("'{}' is not a valid command", content);
                return;
            }
        } else if !is_emoji(stripped_content.chars()) {
            debug!("'{}' is not ascii or emoji", content);
            return;
        }

        let redis_connection_mutex = match data.get::<RedisConnectionContainer>() {
            Some(con) => con.clone(),
            None => panic!("Didn't find RedisConnectionContainer within the context"),
        };
        let redis_connection = &mut *redis_connection_mutex.lock();

        let id = match message.guild_id {
            Some(guild_id) => format!("guild.{}", guild_id),
            None => format!("user.{}", message.author.id),
        };

        let response = match redis::cmd(redis_command)
            .arg(format!("{}.{}", id, content))
            .query(redis_connection)
        {
            Ok(response) => response,
            Err(error) => {
                warn!("Error on running redis query: {}", error);
                return;
            }
        };

        match response {
            None => (),
            Some(response) => {
                let new_val: isize = response;
                debug!("'{}' has a new value of '{}'", content, new_val);
                let _ = message.channel_id.send_message(&ctx, |m| {
                    m.content(format!("{} ⟶ {}", content, new_val));
                    m
                });
            }
        };
    }
}

fn get_env_var(env_var: &str, default: Option<&str>) -> String {
    match env::var(env_var) {
        Ok(val) => val,
        Err(error) => {
            if let Some(default) = default {
                info!("{} not set, defaulting to {}", env_var, default);
                default.to_string()
            } else {
                warn!("{} not set, no default: {}", env_var, error);
                process::exit(1);
            }
        }
    }
}

fn get_redis_connection_info() -> redis::ConnectionInfo {
    redis::ConnectionInfo {
        addr: Box::new(redis::ConnectionAddr::Tcp(
            get_env_var("INCYDECY_REDIS_HOST", Some("127.0.0.1")),
            get_env_var("INCYDECY_REDIS_PORT", Some("6379"))
                .parse::<u16>()
                .unwrap_or(6379),
        )),
        db: get_env_var("INCYDECY_REDIS_DATABASE", Some("0"))
            .parse::<i64>()
            .unwrap_or(0),
        passwd: {
            let redis_passwd = get_env_var("INCYDECY_REDIS_PASSWORD", Some(""));
            if redis_passwd == "" {
                None
            } else {
                Some(redis_passwd)
            }
        },
    }
}

fn main() {
    kankyo::init().ok();

    env_logger::init_from_env(
        env_logger::Env::default()
            .filter_or("incydecy", "warn")
            .write_style("inydecy"),
    );

    let mut discord_client = match Client::new(get_env_var("INCYDECY_DISCORD_TOKEN", None), Handler)
    {
        Ok(dc) => dc,
        Err(error) => {
            warn!("Creating Discord client failed: {}", error);
            process::exit(1);
        }
    };

    let redis_client = match redis::Client::open(get_redis_connection_info()) {
        Ok(rc) => rc,
        Err(error) => {
            warn!("Error connecting to redis: {}", error);
            process::exit(1);
        }
    };

    {
        let mut data = discord_client.data.write();
        data.insert::<RedisConnectionContainer>(Arc::new(Mutex::new(
            redis_client
                .get_connection()
                .expect("Couldn't get a connection to Redis"),
        )));
        data.insert::<RegexContainer>(Arc::new(Mutex::new(
            Regex::new(r"^([\w\d<>:]*)$").expect("Invalid regular expression"),
        )));
    }

    if let Err(why) = discord_client.start() {
        println!("An error occurred while running the client: {:?}", why);
    }
}
