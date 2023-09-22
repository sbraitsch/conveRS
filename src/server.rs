#![allow(dead_code)]

use std::collections::HashMap;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;

use chat::chat_server::{Chat, ChatServer};
use chat::{ChatMessage, NameCheckRequest, NameCheckResponse};
use colored::Colorize;
use futures_core::Stream;
use lazy_static::lazy_static;
use tokio::sync::{broadcast, Mutex};
use tonic::{transport::Server, Request, Response, Status};

pub mod message_parser;

pub mod chat {
    tonic::include_proto!("chat");
}

lazy_static! {
    static ref TX: broadcast::Sender<ChatMessage> = {
        let (tx, _) = broadcast::channel(100);
        tx
    };
    static ref USERMAP: Arc<Mutex<HashMap<String, String>>> = {
        let map: HashMap<String, String> = HashMap::new();
        Arc::new(Mutex::from(map))
    };
}

#[derive(Default, Debug)]
pub struct ChatService {}

#[tonic::async_trait]
impl Chat for ChatService {
    type LiveChatStream = Pin<Box<dyn Stream<Item = Result<ChatMessage, Status>> + Send + 'static>>;

    async fn check_for_name(
        &self,
        request: Request<NameCheckRequest>
    ) -> Result<Response<NameCheckResponse>, Status> {
        let guard = USERMAP.lock().await;
        let available = !guard.contains_key(&request.into_inner().name);
        Ok(Response::new(NameCheckResponse { available }))
    }

    async fn live_chat(
        &self,
        request: Request<tonic::Streaming<ChatMessage>>,
    ) -> Result<Response<Self::LiveChatStream>, Status> {
        let mut input_stream = request.into_inner();
        let room = Arc::new(Mutex::new(String::new()));
        let room_copy = room.clone();
        let user = Arc::new(Mutex::new(String::new()));
        let user_copy = user.clone();

        tokio::spawn(async move {
            while let Ok(Some(message)) = input_stream.message().await {
                let mut user_guard = user.lock().await;
                *user_guard = message.sender.clone();
                let mut room_guard = room.lock().await;
                *room_guard = message.chatroom.clone();

                {
                    let mut guard = USERMAP.lock().await;
                    let server_response = ChatMessage::into_response(message, &mut guard, &*TX).await;
                    *room_guard = server_response.chatroom.clone();
                    let _ = TX.send(server_response);
                }
            }
            {
                let user_guard = user.lock().await;
                let room_guard = room.lock().await;
                remove_user_from_map(&user_guard).await;
                send_disconnect_message(&room_guard, &user_guard);
            }
        });

        let output_stream = async_stream::try_stream! {
            while let Ok(message) = TX.subscribe().recv().await {
                {
                    let room = room_copy.lock().await;
                    let user = user_copy.lock().await;
                    if (message.target.is_empty()) {
                        if message.chatroom == *room {
                            yield message;
                        }
                    } else if (message.target == *user || message.sender == *user) {
                        yield message;
                    }
                }
            }
        };

        Ok(Response::new(
            Box::pin(output_stream) as Self::LiveChatStream
        ))
    }
}

async fn remove_user_from_map(user: &String) {
    let mut guard = USERMAP.lock().await;
    guard.remove(user);
}

fn send_disconnect_message(room: &String, user: &String) {
    let _ = TX.send(
        ChatMessage { 
            sender: "server".to_string(), 
            timestamp: timestamp(), 
            chatroom: room.to_string(), 
            content: format!("-- {} has left.", user).truecolor(153, 140, 139).to_string(),
            target: String::new()
        }
    );
}

pub fn timestamp() -> i64 {
    let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time broke..");
    time.as_millis() as i64    
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse().unwrap();
    let service = ChatService::default();
    
    print!("\x1B[2J\x1B[1;1H");
    println!("GrpcServer listening on {}", addr);

    Server::builder()
        .add_service(ChatServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
