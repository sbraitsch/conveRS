#![allow(dead_code)]

use colored::*;
use futures_util::{stream, StreamExt};
use reqwest::{Error, Response};
use serde::Deserialize;
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;

use super::ChatMessage;

#[derive(Deserialize)]
struct Currency {
    symbol: String,
    price: String,
}

#[derive(Deserialize, Clone)]
struct HNStory {
    by: Option<String>,
    score: i16,
    time: i64,
    title: String,
    url: Option<String>,
}

impl ChatMessage {
    pub async fn into_response(
        inbound: ChatMessage,
        users: &mut tokio::sync::MutexGuard<'_, HashMap<String, String>>,
        tx: &broadcast::Sender<ChatMessage>,
    ) -> ChatMessage {
        if inbound.chatroom.is_empty() && !inbound.content.starts_with("!join ") {
            return build_need_to_join_response(inbound);
        }
        let content_copy = inbound.content.clone();
        match content_copy.as_str() {
            "!user" => build_user_command_response(inbound, users),
            "!news" => build_hn_command_response(inbound, tx).await,
            s if s.starts_with("!join ") => build_user_connection_response(s, users, inbound),
            s if s.starts_with("!dm ") => build_direct_message_response(s, users, inbound),
            s if s.starts_with("!value ") => build_binance_command_response(s, inbound).await,
            _ => inbound,
        }
    }
}

fn build_need_to_join_response(mut inbound: ChatMessage) -> ChatMessage {
    inbound.target = inbound.sender;
    inbound.sender = "server".to_string();
    inbound.content = format!(
        "Type {} to enter a room.",
        String::from("!join <roomname>").bright_yellow()
    );
    inbound
}

async fn build_binance_command_response(s: &str, mut inbound: ChatMessage) -> ChatMessage {
    let currency = s.strip_prefix("!value ").unwrap().to_uppercase();
    let url = format!(
        "https://api4.binance.com/api/v3/ticker/price?symbol={}EUR",
        currency
    );
    if let Ok(response) = get_currency_conversion(url).await {
        if response.status().is_server_error() || response.status().is_client_error() {
            inbound.content = format!(
                "{} {}",
                String::from("Error requesting conversion rate for").red(),
                currency.bright_yellow()
            );
        } else {
            if let Ok(value) = response.text().await {
                let parsed_response: Currency = serde_json::from_str(&value).unwrap();
                let price = parsed_response.price.parse::<f32>().unwrap();
                inbound.content = format!(
                    "{}{} is currently worth {}{}",
                    String::from("$").bright_yellow(),
                    currency.bright_yellow(),
                    format!("{price:.2}").bright_yellow(),
                    String::from("€").bright_yellow()
                );
            }
        }
    } else {
        inbound.content = format!(
            "{}",
            String::from("Something went wrong when trying to retrieve currency information").red()
        );
    }
    inbound.sender = String::from("server");
    inbound
}

async fn build_hn_command_response(
    mut inbound: ChatMessage,
    tx: &broadcast::Sender<ChatMessage>,
) -> ChatMessage {
    inbound.target = inbound.sender.clone();
    inbound.sender = String::from("server");
    let tx = tx.clone();
    let mut delayed = inbound.clone();
    tokio::spawn(async move {
        let url = "https://hacker-news.firebaseio.com/v0/topstories.json?print=pretty".to_string();
        if let Ok(top) = get_top_hn(url).await {
            delayed.content = top
                .iter()
                .map(|story| {
                    format!(
                        "{} {} \t\"{}\" {} {}\n\t{}",
                        String::from("▲").truecolor(100, 248, 140),
                        story.score.to_string().truecolor(100, 248, 140),
                        story.title.white(),
                        String::from("by").truecolor(153, 140, 139),
                        story
                            .by
                            .clone()
                            .unwrap_or(String::new())
                            .truecolor(100, 248, 140),
                        story
                            .url
                            .clone()
                            .unwrap_or(String::new())
                            .truecolor(99, 215, 253)
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");
        } else {
            delayed.content = format!(
                "{}",
                String::from("Couldn't retrieve Hacker News frontpage").red()
            );
        }
        let _ = tx.send(delayed);
    });
    inbound.content = format!(
        "{}",
        String::from("Retrieving News ...").truecolor(153, 140, 139)
    );
    inbound
}

fn build_direct_message_response(
    s: &str,
    users: &mut tokio::sync::MutexGuard<HashMap<String, String>>,
    inbound: ChatMessage,
) -> ChatMessage {
    let cmd_split = s.split(" ").collect::<Vec<&str>>();
    let target_user = cmd_split.get(1).unwrap();
    let msg = &cmd_split[2..];

    if !users.contains_key(target_user.to_owned()) {
        ChatMessage {
            sender: "server".to_string(),
            timestamp: timestamp(),
            chatroom: inbound.chatroom,
            content: format!(
                "No user named {} is currently connected.",
                target_user.bright_yellow()
            ),
            target: inbound.sender,
        }
    } else {
        ChatMessage {
            sender: inbound.sender,
            timestamp: timestamp(),
            chatroom: inbound.chatroom,
            content: msg.join(" "),
            target: target_user.to_string(),
        }
    }
}

fn build_user_connection_response(
    s: &str,
    users: &mut tokio::sync::MutexGuard<HashMap<String, String>>,
    inbound: ChatMessage,
) -> ChatMessage {
    let new_room = s.strip_prefix("!join ").unwrap();
    users.insert(inbound.sender.to_string(), new_room.to_string());
    ChatMessage {
        sender: "server".to_string(),
        timestamp: timestamp(),
        chatroom: new_room.to_string(),
        content: format!("-- {} has joined {}", inbound.sender, new_room)
            .truecolor(153, 140, 139)
            .to_string(),
        target: String::new(),
    }
}

fn build_user_command_response(
    mut inbound: ChatMessage,
    users: &mut tokio::sync::MutexGuard<HashMap<String, String>>,
) -> ChatMessage {
    inbound.target = inbound.sender.clone();
    inbound.sender = String::from("server");
    let users_in_room = users
        .iter()
        .filter(|e| e.1 == inbound.chatroom.as_str())
        .map(|(user, _)| user.clone())
        .collect::<Vec<String>>();
    inbound.content = format!(
        "-- Users in {}: {}",
        inbound.chatroom.bright_cyan(),
        users_in_room.join(", ").bright_yellow()
    );
    inbound
}

fn timestamp() -> i64 {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time broke..");
    time.as_millis() as i64
}

async fn get_currency_conversion(url: String) -> Result<Response, Error> {
    let response = reqwest::get(url).await?;
    Ok(response)
}

async fn get_top_hn(url: String) -> Result<Vec<HNStory>, Error> {
    let response = reqwest::get(url).await?;
    let response = response.text().await?;

    let ids = response
        .trim_start_matches("[ ")
        .trim_end_matches(" ]")
        .split(',')
        .map(|id| id.trim().to_string())
        .take(10)
        .collect::<Vec<String>>();

    let responses = stream::iter(ids)
    .map(|id| async move { get_story_info(&id).await })
    .buffer_unordered(10)
    .collect::<Vec<Result<HNStory, reqwest::Error>>>()
    .await;

    Ok(responses.into_iter().filter_map(Result::ok).collect())
}

async fn get_story_info(id: &str) -> Result<HNStory, Error> {
    let story_url = format!(
        "https://hacker-news.firebaseio.com/v0/item/{}.json?print=pretty",
        id
    );
    let story_response = reqwest::get(story_url).await?;
    let story_data = story_response.text().await?;
    let parsed_story: HNStory = serde_json::from_str(&story_data).unwrap();
    Ok(parsed_story)
}
