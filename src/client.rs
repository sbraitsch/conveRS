use std::env;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chat::chat_client::ChatClient;
use chat::{ChatMessage, NameCheckRequest};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tonic::Request;

use chrono::naive::NaiveDateTime;
use colored::*;

pub mod chat {
    tonic::include_proto!("chat");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = ChatClient::connect(resolve_server_ip()).await?;

    print!("\x1B[2J\x1B[1;1H");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut user: String;
    println!("Choose a temporary user name:");

    loop {
        let name = reader.next_line().await?.unwrap();
        user = name.as_str().trim_end().to_string();
        if let Ok(response) = client
            .check_for_name(NameCheckRequest { name: user.clone() })
            .await
        {
            if response.into_inner().available {
                break;
            };
        };
        println!("{}", String::from("That name is currently in use.").red());
    }

    let shared_room = Arc::new(Mutex::new(String::new()));
    let room_copy = shared_room.clone();

    let join_default = ChatMessage {
        sender: user.to_string(),
        timestamp: get_time_as_millis(),
        chatroom: String::new(),
        content: String::from("!join public"),
        target: String::new(),
    };

    let outbound = async_stream::stream! {
        
        yield join_default;

        while let Ok(Some(line)) = reader.next_line().await {

            let mut room;
            {
                let mut guard = room_copy.lock().await;
                room = guard.clone();
            }

            let line = line.trim_end().to_string();

            let message = ChatMessage {
                sender: user.to_string(),
                timestamp: get_time_as_millis(),
                chatroom: room,
                content: line.trim().to_string(),
                target: String::new()
            };
            yield message;
        }
    };

    let response = client.live_chat(Request::new(outbound)).await?;
    let mut inbound = response.into_inner();

    while let Some(message) = inbound.message().await? {
        {
            let mut guard = shared_room.lock().await;
            if message.chatroom != *guard && message.target.is_empty() {
                *guard = message.chatroom.clone();
                print!("\x1B[2J\x1B[1;1H");
                print_command_legend();
                println!("\n<{}>", message.chatroom.truecolor(100, 248, 140));
            }
        }
        match message.sender.as_str() {
            "server" => println!("{}", message.content),
            _ => print_user_message(message),
        }
    }

    Ok(())
}

fn print_user_message(message: ChatMessage) {
    if message.target.is_empty() {
        println!(
            "{} {}: {}",
            NaiveDateTime::from_timestamp_millis(message.timestamp)
                .unwrap()
                .format("%H:%M:%S")
                .to_string()
                .truecolor(153, 140, 139),
            message.sender.truecolor(123, 201, 107),
            message.content.white()
        );
    } else {
        println!(
            "{} {} -> {}: {}",
            NaiveDateTime::from_timestamp_millis(message.timestamp)
                .unwrap()
                .format("%H:%M:%S")
                .to_string()
                .truecolor(153, 140, 139),
            message.sender.truecolor(123, 201, 107),
            message.target.truecolor(123, 201, 107),
            message.content.white()
        );
    }
}

fn get_time_as_millis() -> i64 {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time broke..");

    time.as_millis() as i64
}

fn resolve_server_ip() -> String {
    let args: Vec<String> = env::args().collect();
    //let default_server = "http://82.165.184.129:50051".to_string();
    let default_server = "http://[::1]:50051".to_string();
    let server_ip = args.get(1).unwrap_or(&default_server);
    server_ip.to_string()
}

fn print_command_legend() {
    println!(
        "{} {}.",
        "!join <room>".bright_yellow(),
        "to join a room".truecolor(153, 140, 139)
    );
    println!(
        "{} {}.",
        "!user".bright_yellow(),
        "to list all users in this room".truecolor(153, 140, 139)
    );
    println!(
        "{} {}.",
        "!value <tag>".bright_yellow(),
        "to list all users in this room".truecolor(153, 140, 139)
    );
    println!(
        "{} {}.",
        "!dm <user> <message>".bright_yellow(),
        "to send a private message to another user".truecolor(153, 140, 139)
    );
    println!(
        "{} {}.",
        "!news".bright_yellow(),
        "to see a list of the top 10 posts on HN".truecolor(153, 140, 139)
    );
}
