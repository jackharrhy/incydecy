extern crate env_logger;
use log::debug;

use std::env;
use std::str::Chars;
use std::sync::Arc;

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
            debug!("ignoring a message from '{}' since its a bot", message.author);
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
            "++" => Some("INCR"),
            "--" => Some("DECR"),
            _ => None,
        };

        if redis_command.is_none() {
            debug!("'{}' is not a redis command", content);
            return;
        }

        let content = &content[..content.len() - 2];
        let stripped_content = remove_whitespace(content);

        let mut data = ctx.data.write();

        if content.is_ascii() {
            let is_valid_command = data.get_mut::<RegexContainer>().unwrap().clone();
            let is_valid_command = &mut *is_valid_command.lock();

            if !is_valid_command.is_match(&content) {
                debug!("'{}' is not a valid command", content);
                return;
            }
        } else if !is_emoji(stripped_content.chars()) {
            debug!("'{}' is not ascii or emoji", content);
            return;
        }

        let redis_connection = data.get_mut::<RedisConnectionContainer>().unwrap().clone();

        let redis_connection = &mut *redis_connection.lock();

        let guild_id = match message.guild_id {
            Some(guild_id) => guild_id,
            None => return,
        };

        let redis_command = redis_command.unwrap();

        match redis::cmd(redis_command)
            .arg(format!("{}:{}", guild_id, content))
            .query(redis_connection)
            .unwrap()
        {
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

fn main() {
    env_logger::init();
    kankyo::init().expect("Failed to initialize kanko!");

    let mut discord_client = Client::new(
        &env::var("INCYDECY_DISCORD_TOKEN")
            .expect("Couldn't get the INCYDECY_DISCORD_TOKEN variable"),
        Handler,
    )
    .expect("Error creating client");

    let redis_client =
        redis::Client::open("redis://127.0.0.1/").expect("Couldn't connect to Redis");

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
