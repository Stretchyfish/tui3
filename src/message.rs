use serde_json::{Value, json};

#[derive(Clone)]
pub struct SendRequest {
    pub thread_id: String,
    pub command_id: String,
    pub message_id: String,
    pub text: String,
}

impl SendRequest {
    pub fn new(thread_id: String, text: String) -> Self {
        Self {
            thread_id,
            text,
            command_id: uuid::Uuid::new_v4().to_string(),
            message_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn payload(&self) -> Value {
        json!({
            "type": "message.dispatch", "commandId": self.command_id,
            "threadId": self.thread_id, "messageId": self.message_id,
            "text": self.text, "attachments": [], "createdBy": "user", "creationSource": "web",
            "dispatchMode": { "type": "queue_after_active" }
        })
    }
}

#[derive(Debug)]
pub struct SendFailure {
    pub message: String,
    pub retry_same_id: bool,
}

impl SendFailure {
    pub fn uncertain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retry_same_id: true,
        }
    }
}

impl From<String> for SendFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_same_id: false,
        }
    }
}

impl From<&str> for SendFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
