use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::app::{ChatMessage, ChatRole};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

fn build_system_prompt(indicators_json: &str) -> String {
    format!(
        "You are a stock analysis assistant. Answer the user's questions about the stock using \
the provided JSON snapshot. If the question cannot be answered from the data, say so and keep \
the response concise (2–6 sentences).\n\nStock data (JSON):\n{indicators_json}"
    )
}

fn to_message(chat: &ChatMessage) -> Message {
    let role = match chat.role {
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    };
    Message {
        role: role.to_string(),
        content: chat.content.clone(),
    }
}

/// Sends `indicators_json` (pre-serialised StockIndicators) to the OpenRouter chat
/// completions endpoint and returns the model's analysis text.
pub async fn analyze_stock(indicators_json: &str, api_key: &str) -> Result<String, String> {
    let prompt = format!(
        "You are an expert fundamental stock analyst. You have read Benjamin Graham, \
        Philip Fisher, Aswath Damodaran, and other valuation-based analysts, and take them as \
        inspiration for stock analysis. Based on the following stock indicators (JSON), \
        write a concise investment analysis (4–6 sentences) covering valuation, financial health, \
        profitability, and growth. Be direct and opinionated — state whether the stock looks \
        attractive, fairly valued, or expensive, and why.\n\nStock data:\n{indicators_json}"
    );

    let request = ChatRequest {
        model: std::env::var("OPENROUTER_MODEL")
            .unwrap_or("arcee-ai/trinity-large-preview:free".to_string()),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
    };

    let client = Client::new();
    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenRouter {status}: {body}"));
    }

    let chat_response: ChatResponse = response.json().await.map_err(|e| e.to_string())?;

    chat_response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "No response from AI".to_string())
}

/// Sends a chat conversation to the OpenRouter chat completions endpoint and returns
/// the assistant's reply.
pub async fn chat_about_stock(
    indicators_json: &str,
    history: &[ChatMessage],
    api_key: &str,
) -> Result<String, String> {
    let mut messages = Vec::with_capacity(history.len() + 1);
    messages.push(Message {
        role: "system".to_string(),
        content: build_system_prompt(indicators_json),
    });
    messages.extend(history.iter().map(to_message));

    let request = ChatRequest {
        model: std::env::var("OPENROUTER_MODEL")
            .unwrap_or("arcee-ai/trinity-large-preview:free".to_string()),
        messages,
    };

    let client = Client::new();
    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenRouter {status}: {body}"));
    }

    let chat_response: ChatResponse = response.json().await.map_err(|e| e.to_string())?;

    chat_response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "No response from AI".to_string())
}
