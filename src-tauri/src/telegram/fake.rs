//! A scripted `Bot` for tests: records what was sent and fails on demand.
use super::bot::{Bot, BotError, Me, Update};
use std::{collections::VecDeque, sync::Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum Sent {
    Text { chat: i64, text: String },
    Photo { chat: i64, caption: String, bytes: Vec<u8> },
}

#[derive(Default)]
pub struct FakeBot {
    sent: Mutex<Vec<Sent>>,
    /// Errors handed out by the next send calls, in order; each entry is (calls to let through first, error).
    failures: Mutex<VecDeque<(usize, BotError)>>,
    me: Mutex<Option<Result<String, BotError>>>,
}

impl FakeBot {
    pub fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }

    pub fn fail_next(&self, error: BotError) {
        self.fail_after(0, error);
    }

    pub fn fail_after(&self, ok_first: usize, error: BotError) {
        self.failures.lock().unwrap().push_back((ok_first, error));
    }

    pub fn answer_me(&self, answer: Result<&str, BotError>) {
        *self.me.lock().unwrap() = Some(answer.map(str::to_string));
    }

    fn record(&self, sent: Sent) -> Result<(), BotError> {
        let mut failures = self.failures.lock().unwrap();
        if let Some((skip, _)) = failures.front_mut() {
            if *skip == 0 {
                return Err(failures.pop_front().expect("front exists").1);
            }
            *skip -= 1;
        }
        self.sent.lock().unwrap().push(sent);
        Ok(())
    }
}

impl Bot for FakeBot {
    fn get_me(&self, _: &str) -> Result<Me, BotError> {
        match self.me.lock().unwrap().clone() {
            Some(Ok(username)) => Ok(Me { username }),
            Some(Err(e)) => Err(e),
            None => Ok(Me { username: "porteiro_bot".into() }),
        }
    }

    fn get_updates(&self, _: &str, _: i64, _: u64) -> Result<Vec<Update>, BotError> {
        Ok(Vec::new())
    }

    fn send_message(&self, _: &str, chat: i64, text: &str) -> Result<(), BotError> {
        self.record(Sent::Text { chat, text: text.to_string() })
    }

    fn send_photo(&self, _: &str, chat: i64, caption: &str, jpeg: Vec<u8>) -> Result<(), BotError> {
        self.record(Sent::Photo { chat, caption: caption.to_string(), bytes: jpeg })
    }
}
