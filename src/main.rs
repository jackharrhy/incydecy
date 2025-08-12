use std::env;
use std::sync::{Arc, Mutex};

use regex::Regex;
use rusqlite::{Connection, Result as SqlResult};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::prelude::*;

struct Handler {
    db: Arc<Mutex<Connection>>,
    regex: Regex,
}

fn init_database() -> SqlResult<Connection> {
    let conn = Connection::open("./data/incydecy.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS value (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            guild_id TEXT NOT NULL,
            thing TEXT NOT NULL,
            current_value INTEGER DEFAULT 0,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(guild_id, thing)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            guild_id TEXT NOT NULL,
            channel_id TEXT,
            author_id TEXT,
            content TEXT,
            time_sent TIMESTAMP,
            thing TEXT,
            effect INTEGER,
            value_id INTEGER,
            FOREIGN KEY (value_id) REFERENCES value (id)
        )",
        [],
    )?;

    Ok(conn)
}

fn get_leaderboard(conn: &Connection, guild_id: &str) -> SqlResult<Vec<(String, i32)>> {
    let mut stmt = conn.prepare(
        "SELECT thing, current_value FROM value
         WHERE guild_id = ?
         ORDER BY current_value DESC
         LIMIT 10",
    )?;

    let rows = stmt.query_map([guild_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    })?;

    let mut leaderboard = Vec::new();
    for row in rows {
        leaderboard.push(row?);
    }

    Ok(leaderboard)
}

fn get_user_leaderboard(conn: &Connection, guild_id: &str) -> SqlResult<Vec<(String, i32)>> {
    let mut stmt = conn.prepare(
        "SELECT author_id, COUNT(*) as invocation_count FROM messages
         WHERE guild_id = ?
         GROUP BY author_id
         ORDER BY invocation_count DESC
         LIMIT 10",
    )?;

    let rows = stmt.query_map([guild_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    })?;

    let mut leaderboard = Vec::new();
    for row in rows {
        leaderboard.push(row?);
    }

    Ok(leaderboard)
}

fn process_increment_decrement(
    conn: &Connection,
    guild_id: &str,
    channel_id: &str,
    author_id: &str,
    message_id: &str,
    content: &str,
    thing: &str,
    effect: i32,
) -> SqlResult<i32> {
    let tx = conn.unchecked_transaction()?;

    let mut stmt = tx.prepare(
        "INSERT INTO value (guild_id, thing, current_value, created_at, updated_at) 
         VALUES (?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(guild_id, thing) DO UPDATE SET 
             current_value = current_value + ?,
             updated_at = CURRENT_TIMESTAMP
         RETURNING current_value, id",
    )?;

    let (new_value, value_id): (i32, i64) = stmt.query_row(
        [guild_id, thing, &effect.to_string(), &effect.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    drop(stmt);

    tx.execute(
        "INSERT OR REPLACE INTO messages (id, guild_id, channel_id, author_id, content, time_sent, thing, effect, value_id)
         VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?, ?, ?)",
        [message_id, guild_id, channel_id, author_id, content, thing, &effect.to_string(), &value_id.to_string()]
    )?;

    tx.commit()?;

    Ok(new_value)
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if let Some(guild_id) = msg.guild_id {
            if msg.mentions_me(&ctx.http).await.unwrap_or(false) {
                let content_lower = msg.content.to_lowercase();
                let db = self.db.clone();
                let guild_id_str = guild_id.to_string();

                if content_lower.contains("user") && content_lower.contains("leaderboard") {
                    let result = {
                        match db.lock() {
                            Ok(conn) => get_user_leaderboard(&conn, &guild_id_str),
                            Err(why) => {
                                println!("Failed to acquire database lock: {why:?}");
                                return;
                            }
                        }
                    };

                    match result {
                        Ok(leaderboard) => {
                            let mut response = String::from("**User Leaderboard:**\n");
                            for (i, (user_id, count)) in leaderboard.iter().enumerate() {
                                response.push_str(&format!(
                                    "{}. <@{}> ⟶ {} invocations\n",
                                    i + 1,
                                    user_id,
                                    count
                                ));
                            }

                            if leaderboard.is_empty() {
                                response = String::from("No user activity tracked yet!");
                            }

                            if let Err(why) = msg.channel_id.say(&ctx.http, &response).await {
                                println!("Error sending user leaderboard: {why:?}");
                            }
                        }
                        Err(why) => {
                            println!("Database error getting user leaderboard: {why:?}");
                        }
                    }
                } else if content_lower.contains("leaderboard") {
                    let result = {
                        match db.lock() {
                            Ok(conn) => get_leaderboard(&conn, &guild_id_str),
                            Err(why) => {
                                println!("Failed to acquire database lock: {why:?}");
                                return;
                            }
                        }
                    };

                    match result {
                        Ok(leaderboard) => {
                            let mut response = String::from("**Leaderboard:**\n");
                            for (i, (thing, value)) in leaderboard.iter().enumerate() {
                                response.push_str(&format!("{}. {} ⟶ {}\n", i + 1, thing, value));
                            }

                            if leaderboard.is_empty() {
                                response = String::from("No values tracked yet!");
                            }

                            if let Err(why) = msg.channel_id.say(&ctx.http, &response).await {
                                println!("Error sending leaderboard: {why:?}");
                            }
                        }
                        Err(why) => {
                            println!("Database error getting leaderboard: {why:?}");
                        }
                    }
                }
            } else if let Some(captures) = self.regex.captures(&msg.content) {
                let thing = &captures[1];
                let operation = &captures[2];
                let effect = if operation == "++" { 1 } else { -1 };

                println!("Processing message: {thing} {operation}");

                let db = self.db.clone();
                let guild_id_str = guild_id.to_string();
                let channel_id_str = msg.channel_id.to_string();
                let author_id_str = msg.author.id.to_string();
                let message_id_str = msg.id.to_string();
                let content = msg.content.clone();
                let thing_owned = thing.to_string();

                let result = {
                    match db.lock() {
                        Ok(conn) => process_increment_decrement(
                            &conn,
                            &guild_id_str,
                            &channel_id_str,
                            &author_id_str,
                            &message_id_str,
                            &content,
                            &thing_owned,
                            effect,
                        ),
                        Err(why) => {
                            println!("Failed to acquire database lock: {why:?}");
                            return;
                        }
                    }
                };

                match result {
                    Ok(new_value) => {
                        println!("Processed message: {thing} {operation} ⟶ {}", new_value);

                        let response = format!("{} ⟶ {}", thing, new_value);
                        if let Err(why) = msg.channel_id.say(&ctx.http, &response).await {
                            println!("Error sending message: {why:?}");
                        }
                    }
                    Err(why) => {
                        println!("Database error: {why:?}");
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();

    let conn = init_database().expect("Failed to initialize database");
    let db = Arc::new(Mutex::new(conn));

    let regex = Regex::new(r"^(\S+)(\+\+|--)$").expect("Failed to compile regex");

    let token = env::var("INCYDECY_DISCORD_TOKEN").expect("Expected a token in the environment");

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler { db, regex };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("Error creating client");

    println!("Starting incydecy");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}
