import duckdb
import re
import sqlite3
from datetime import datetime


GUILD_ID = "497544520695808000"


def init_tables(sqlite_db="incydecy.db"):
    conn = sqlite3.connect(sqlite_db)
    cursor = conn.cursor()

    # Create value table
    cursor.execute("""
        CREATE TABLE IF NOT EXISTS value (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            guild_id TEXT NOT NULL,
            thing TEXT NOT NULL,
            current_value INTEGER DEFAULT 0,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(guild_id, thing)
        )
    """)

    # Create messages table
    cursor.execute("""
        CREATE TABLE IF NOT EXISTS messages (
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
        )
    """)

    conn.commit()
    conn.close()


def process_messages_in_chunks(db_path="activity.duckdb", chunk_size=1000):
    conn = duckdb.connect(db_path)

    total_messages = conn.execute("SELECT COUNT(*) FROM messages").fetchone()[0]
    print(f"Processing {total_messages} messages in chunks of {chunk_size}")

    incy_decy_messages = {
        "positive": [],
        "negative": [],
    }

    values = {}

    offset = 0
    while offset < total_messages:
        chunk = conn.execute(
            "SELECT id, channel_id, author_id, content, time_sent FROM messages LIMIT ? OFFSET ?",
            [chunk_size, offset],
        ).fetchall()

        if not chunk:
            break

        print(f"Processing chunk {offset // chunk_size + 1}: {len(chunk)} messages")

        for message in chunk:
            message_id, channel_id, author_id, content, time_sent = message

            if content and re.search(r"^\S+\+\+$", content.strip()):
                thing = content.strip()[:-2]
                values[thing] = values.get(thing, 0) + 1

                incy_decy_messages["positive"].append(
                    {
                        "id": message_id,
                        "channel_id": channel_id,
                        "author_id": author_id,
                        "content": content,
                        "time_sent": time_sent,
                        "thing": thing,
                    }
                )
            elif content and re.search(r"^\S+--$", content.strip()):
                thing = content.strip()[:-2]
                values[thing] = values.get(thing, 0) - 1

                incy_decy_messages["negative"].append(
                    {
                        "id": message_id,
                        "channel_id": channel_id,
                        "author_id": author_id,
                        "content": content,
                        "time_sent": time_sent,
                        "thing": thing,
                    }
                )
        offset += chunk_size

    conn.close()

    print(f"\nFound {len(incy_decy_messages['positive'])} positive")
    print(f"Found {len(incy_decy_messages['negative'])} negative")
    print(f"Tracked values for {len(values)} different things")

    return incy_decy_messages, values


def save_to_sqlite(messages, values, sqlite_db="incydecy.db"):
    conn = sqlite3.connect(sqlite_db)
    cursor = conn.cursor()

    try:
        conn.execute("BEGIN TRANSACTION")

        value_ids = {}
        for thing, current_value in values.items():
            now = datetime.now().isoformat()
            cursor.execute(
                """
                INSERT INTO value (guild_id, thing, current_value, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(guild_id, thing) DO UPDATE SET
                    current_value = excluded.current_value,
                    updated_at = excluded.updated_at
            """,
                (GUILD_ID, thing, current_value, now, now),
            )

            cursor.execute(
                "SELECT id FROM value WHERE guild_id = ? AND thing = ?",
                (GUILD_ID, thing),
            )
            value_ids[thing] = cursor.fetchone()[0]

        # Insert messages
        all_messages = messages["positive"] + messages["negative"]
        for msg in all_messages:
            effect = 1 if msg in messages["positive"] else -1
            time_sent = msg["time_sent"]
            if hasattr(time_sent, "isoformat"):
                time_sent = time_sent.isoformat()

            cursor.execute(
                """
                INSERT OR REPLACE INTO messages 
                (id, guild_id, channel_id, author_id, content, time_sent, thing, effect, value_id)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    msg["id"],
                    GUILD_ID,
                    msg["channel_id"],
                    msg["author_id"],
                    msg["content"],
                    time_sent,
                    msg["thing"],
                    effect,
                    value_ids[msg["thing"]],
                ),
            )

        conn.commit()
        print(f"Saved {len(all_messages)} messages and {len(values)} values to SQLite")

    except Exception as e:
        conn.rollback()
        print(f"Error saving to SQLite: {e}")
        raise
    finally:
        conn.close()


def main():
    init_tables()

    messages, values = process_messages_in_chunks()

    print(
        f"Messages: {len(messages['positive'])} positive, {len(messages['negative'])} negative"
    )

    sorted_values = sorted(values.items(), key=lambda x: x[1], reverse=True)
    print("\nTop 10 values:")
    for thing, value in sorted_values[:10]:
        print(f"  {thing}: {value}")

    save_to_sqlite(messages, values)


if __name__ == "__main__":
    main()
