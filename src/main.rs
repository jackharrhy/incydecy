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
        let content: &String = &message.content;
        let length = content.chars().count();

        if length >= 50 || length < 3 {
            return;
        }

        let command = match &content[content.len() - 2..content.len()] {
            "++" => Some("INCR"),
            "--" => Some("DECR"),
            _ => None,
        };

        if command.is_none() {
            return;
        }

        let content = &content[..content.len() - 2];
        let stripped_content = remove_whitespace(content);

        if is_emoji(stripped_content.chars()) {

        } else if !(content.is_ascii()) {
            return;
        } else {
            // TODO don't create the regex on every command
            let is_valid_command = Regex::new(r"^([\w\d<>:]*)$").unwrap();

            if !is_valid_command.is_match(&content) {
                return;
            }
        }
        let command = command.unwrap();

        let guild_id = match message.guild_id {
            Some(guild_id) => guild_id,
            None => return,
        };

        let mut data = ctx.data.write();

        let redis_connection = data.get_mut::<RedisConnectionContainer>().unwrap().clone();

        let redis_connection = &mut *redis_connection.lock();

        println!("key: {}, command: {}", content, command);

        match redis::cmd(command)
            .arg(format!("{}:{}", guild_id, content))
            .query(redis_connection)
            .unwrap()
        {
            None => (),
            Some(response) => {
                let new_val: isize = response;
                println!("New value: {}", new_val);

                let _ = message.channel_id.send_message(&ctx, |m| {
                    m.content(format!("{} ⟶ {}", content, new_val));
                    m
                });
            }
        };
    }
}

fn main() {
    kankyo::init().unwrap();

    let mut discord_client =
        Client::new(&env::var("INCYDECY_DISCORD_TOKEN").expect("token"), Handler)
            .expect("Error creating client");

    let redis_client = redis::Client::open("redis://127.0.0.1/").unwrap();

    {
        let mut data = discord_client.data.write();
        data.insert::<RedisConnectionContainer>(Arc::new(Mutex::new(
            redis_client.get_connection().unwrap(),
        )));
    }

    if let Err(why) = discord_client.start() {
        println!("An error occurred while running the client: {:?}", why);
    }
}
