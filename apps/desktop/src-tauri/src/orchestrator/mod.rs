//! One conversation per popover (docs/PLAN.md 5.3): effort level, message
//! history, the tool loop and its budget, streaming events to the UI, and
//! cancellation. History is append-only: a turn that fails is removed whole,
//! and correcting the target starts a new conversation.

pub mod prompts;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use serde::Serialize;
use tauri::async_runtime::{self, JoinHandle};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::context::ContextPack;
use crate::engines::{
    AskRequest, Block, Effort, Engine, EngineError, EngineEvent, Message, StopReason, ToolSpec,
    Usage,
};
use crate::settings::SettingsStore;

/// A paused server-side tool loop is resumed at most this many times.
const MAX_CONTINUATIONS: u32 = 3;

/// What the popover shows, as a stream of events.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnswerEvent {
    /// The user released the hotkey; the screen is being read.
    #[serde(rename_all = "camelCase")]
    Preparing { ask_mode: bool },
    /// A new conversation. In ask mode nothing is asked until the user types.
    #[serde(rename_all = "camelCase")]
    Started {
        conversation: u64,
        ceiling: Effort,
        awaiting_question: bool,
    },
    /// A new turn. `prompt` is what the user typed, if anything.
    #[serde(rename_all = "camelCase")]
    TurnStarted {
        conversation: u64,
        turn: u32,
        effort: Effort,
        prompt: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Delta {
        conversation: u64,
        turn: u32,
        text: String,
    },
    /// What's happening, for the status line, or None to clear it.
    #[serde(rename_all = "camelCase")]
    Status {
        conversation: u64,
        turn: u32,
        text: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    TurnDone {
        conversation: u64,
        turn: u32,
        /// The answer hit its length cap.
        truncated: bool,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        conversation: u64,
        turn: u32,
        message: String,
        /// Settings can fix it (no key, a rejected key, no credits).
        needs_setup: bool,
    },
}

pub trait AnswerSink: Send + Sync {
    fn send(&self, event: AnswerEvent);
}

/// Local tools the model may call (docs/PLAN.md 7). Code projects get them in M4.
#[async_trait]
pub trait Toolbox: Send + Sync {
    fn specs(&self) -> Vec<ToolSpec>;
    /// Runs a tool: Ok(output) or Err(message for the model).
    async fn run(&self, name: &str, input: &serde_json::Value) -> Result<String, String>;
}

/// No local tools, outside code mode.
pub struct NoTools;

#[async_trait]
impl Toolbox for NoTools {
    fn specs(&self) -> Vec<ToolSpec> {
        Vec::new()
    }

    async fn run(&self, name: &str, _input: &serde_json::Value) -> Result<String, String> {
        Err(format!("There is no tool named {name}."))
    }
}

pub struct Orchestrator {
    engine: Arc<dyn Engine>,
    settings: Arc<SettingsStore>,
    sink: Arc<dyn AnswerSink>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    current: Option<Conversation>,
}

struct Conversation {
    id: u64,
    pack: Arc<ContextPack>,
    history: Vec<Message>,
    effort: Effort,
    turns: u32,
    busy: bool,
    cancel: CancellationToken,
    toolbox: Arc<dyn Toolbox>,
}

impl Orchestrator {
    pub fn new(
        engine: Arc<dyn Engine>,
        settings: Arc<SettingsStore>,
        sink: Arc<dyn AnswerSink>,
    ) -> Self {
        Self {
            engine,
            settings,
            sink,
            state: Mutex::default(),
        }
    }

    /// The popover is opening while the screen is read.
    pub fn preparing(&self, ask_mode: bool) {
        self.cancel();
        self.sink.send(AnswerEvent::Preparing { ask_mode });
    }

    /// Starts a conversation about `pack`. Unless the user is going to type a
    /// question (ask mode), asks the default one right away, at Low effort.
    pub fn begin(self: &Arc<Self>, pack: ContextPack, ask_mode: bool) -> Option<JoinHandle<()>> {
        self.begin_with(Arc::new(pack), Arc::new(NoTools), ask_mode, None)
    }

    /// Re-asks about the same pack, with the user's correction of what they
    /// pointed at. A new conversation, so history is never edited.
    pub fn correct_target(self: &Arc<Self>, target: String) -> Option<JoinHandle<()>> {
        let (pack, toolbox) = {
            let state = self.state();
            let current = state.current.as_ref()?;
            (current.pack.clone(), current.toolbox.clone())
        };
        self.begin_with(pack, toolbox, false, Some(target))
    }

    fn begin_with(
        self: &Arc<Self>,
        pack: Arc<ContextPack>,
        toolbox: Arc<dyn Toolbox>,
        ask_mode: bool,
        correction: Option<String>,
    ) -> Option<JoinHandle<()>> {
        let ceiling = self.settings.get().effort_ceiling;
        let id = {
            let mut state = self.state();
            if let Some(old) = &state.current {
                old.cancel.cancel();
            }
            state.next_id += 1;
            let id = state.next_id;
            state.current = Some(Conversation {
                id,
                pack: pack.clone(),
                history: Vec::new(),
                effort: Effort::Low,
                turns: 0,
                busy: false,
                cancel: CancellationToken::new(),
                toolbox,
            });
            id
        };
        self.sink.send(AnswerEvent::Started {
            conversation: id,
            ceiling,
            awaiting_question: ask_mode,
        });
        if ask_mode {
            return None;
        }
        let message = prompts::first_message(&pack, None, correction.as_deref());
        self.start_turn(message, Effort::Low, None)
    }

    /// The user's own question: the first one in ask mode, or a follow-up.
    pub fn ask(self: &Arc<Self>, question: String) -> Option<JoinHandle<()>> {
        let (message, effort) = {
            let state = self.state();
            let current = state.current.as_ref()?;
            let message = if current.history.is_empty() {
                prompts::first_message(&current.pack, Some(&question), None)
            } else {
                Message::user_text(question.clone())
            };
            (message, current.effort)
        };
        self.start_turn(message, effort, Some(question))
    }

    /// Re-asks one effort level up, within the ceiling.
    pub fn go_deeper(self: &Arc<Self>) -> Option<JoinHandle<()>> {
        let effort = self.state().current.as_ref()?.effort;
        let deeper = effort.deeper(self.settings.get().effort_ceiling)?;
        self.set_effort(deeper)
    }

    /// Re-asks at a chosen effort, within the ceiling.
    pub fn set_effort(self: &Arc<Self>, effort: Effort) -> Option<JoinHandle<()>> {
        if effort > self.settings.get().effort_ceiling {
            return None;
        }
        self.start_turn(prompts::go_deeper(), effort, None)
    }

    /// Stops whatever is streaming, for example when the popover closes.
    pub fn cancel(&self) {
        if let Some(current) = &self.state().current {
            current.cancel.cancel();
        }
    }

    /// What the current conversation is about, for "What was sent".
    pub fn pack(&self) -> Option<Arc<ContextPack>> {
        self.state().current.as_ref().map(|c| c.pack.clone())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn start_turn(
        self: &Arc<Self>,
        message: Message,
        effort: Effort,
        prompt: Option<String>,
    ) -> Option<JoinHandle<()>> {
        let (id, turn, cancel, rollback) = {
            let mut state = self.state();
            let current = state.current.as_mut()?;
            if current.busy {
                return None;
            }
            // A cancelled conversation (the popover closed) gets a fresh token.
            if current.cancel.is_cancelled() {
                current.cancel = CancellationToken::new();
            }
            current.busy = true;
            current.effort = effort;
            current.turns += 1;
            let rollback = current.history.len();
            current.history.push(message);
            (current.id, current.turns, current.cancel.clone(), rollback)
        };
        self.sink.send(AnswerEvent::TurnStarted {
            conversation: id,
            turn,
            effort,
            prompt,
        });
        let this = self.clone();
        Some(async_runtime::spawn(async move {
            let result = this.run_turn(id, turn, effort, &cancel).await;
            this.finish_turn(id, turn, rollback, result);
        }))
    }

    /// Calls the engine until the model is done, running local tools on the
    /// way while the budget lasts.
    async fn run_turn(
        &self,
        id: u64,
        turn: u32,
        effort: Effort,
        cancel: &CancellationToken,
    ) -> Result<StopReason, EngineError> {
        let settings = self.settings.get();
        let mut tools_left = self.engine.limits(effort).tool_budget;
        let mut continuations = 0;
        loop {
            let (messages, source, toolbox) = {
                let state = self.state();
                let current = match &state.current {
                    Some(c) if c.id == id => c,
                    _ => return Err(EngineError::Cancelled),
                };
                (
                    current.history.clone(),
                    current.pack.source,
                    current.toolbox.clone(),
                )
            };
            let request = AskRequest {
                effort,
                system: prompts::system(source, effort, &settings.language()),
                messages,
                tools: if tools_left > 0 {
                    toolbox.specs()
                } else {
                    Vec::new()
                },
                web_search: true,
            };

            let (events, mut incoming) = mpsc::unbounded_channel();
            let forward = async {
                let mut usage = Usage::default();
                while let Some(event) = incoming.recv().await {
                    match event {
                        EngineEvent::TextDelta(text) => self.sink.send(AnswerEvent::Delta {
                            conversation: id,
                            turn,
                            text,
                        }),
                        EngineEvent::Status(text) => self.status(id, turn, text),
                        EngineEvent::ToolStarted { summary, .. } => {
                            self.status(id, turn, Some(summary))
                        }
                        EngineEvent::ToolFinished { .. } => self.status(id, turn, None),
                        EngineEvent::Usage(u) => usage = u,
                    }
                }
                usage
            };
            let (outcome, usage) =
                tokio::join!(self.engine.ask(request, events, cancel.clone()), forward);
            let outcome = outcome?;
            log::info!(
                "answer turn {turn} at {}: {} in, {} cache write, {} cache read, {} out tokens, ~${:.4}",
                effort.key(),
                usage.input_tokens,
                usage.cache_write_tokens,
                usage.cache_read_tokens,
                usage.output_tokens,
                usage.cost_usd.unwrap_or_default(),
            );

            if outcome.stop == StopReason::Refusal {
                return Ok(StopReason::Refusal);
            }
            let calls: Vec<(String, String, serde_json::Value)> = outcome
                .message
                .tool_uses()
                .map(|(id, name, input)| (id.to_string(), name.to_string(), input.clone()))
                .collect();
            self.append(id, outcome.message)?;

            match outcome.stop {
                StopReason::ToolUse if !calls.is_empty() => {
                    let mut results = Vec::new();
                    for (call_id, name, input) in calls {
                        let result = if tools_left == 0 {
                            Err("The tool budget is used up. Answer now with what you have.".into())
                        } else {
                            tools_left -= 1;
                            self.status(id, turn, Some(format!("Using {name}…")));
                            toolbox_run(&self.toolbox(id)?, &name, &input, cancel).await?
                        };
                        let (content, is_error) = match result {
                            Ok(output) => (output, false),
                            Err(message) => (message, true),
                        };
                        results.push(Block::ToolResult {
                            id: call_id,
                            content,
                            is_error,
                        });
                    }
                    self.append(id, Message::user(results))?;
                }
                StopReason::PauseTurn if continuations < MAX_CONTINUATIONS => continuations += 1,
                stop => return Ok(stop),
            }
        }
    }

    fn toolbox(&self, id: u64) -> Result<Arc<dyn Toolbox>, EngineError> {
        match &self.state().current {
            Some(c) if c.id == id => Ok(c.toolbox.clone()),
            _ => Err(EngineError::Cancelled),
        }
    }

    fn append(&self, id: u64, message: Message) -> Result<(), EngineError> {
        match &mut self.state().current {
            Some(c) if c.id == id => {
                c.history.push(message);
                Ok(())
            }
            _ => Err(EngineError::Cancelled),
        }
    }

    fn status(&self, id: u64, turn: u32, text: Option<String>) {
        self.sink.send(AnswerEvent::Status {
            conversation: id,
            turn,
            text,
        });
    }

    fn finish_turn(
        &self,
        id: u64,
        turn: u32,
        rollback: usize,
        result: Result<StopReason, EngineError>,
    ) {
        {
            let mut state = self.state();
            let Some(current) = state.current.as_mut().filter(|c| c.id == id) else {
                return; // replaced by a newer conversation
            };
            current.busy = false;
            if !matches!(
                result,
                Ok(StopReason::EndTurn | StopReason::MaxTokens | StopReason::Other)
            ) {
                // A turn that didn't finish leaves no trace in the history.
                current.history.truncate(rollback);
            }
        }
        let failure = |message: &str, needs_setup: bool| AnswerEvent::Failed {
            conversation: id,
            turn,
            message: message.to_string(),
            needs_setup,
        };
        let event = match result {
            Ok(StopReason::Refusal) => failure(
                "This one can't be answered here. Try pointing at something else, or ask a different question.",
                false,
            ),
            Ok(StopReason::ToolUse | StopReason::PauseTurn) => failure(
                "The answer took too many steps. Try Go deeper for a bigger budget.",
                false,
            ),
            Ok(stop) => AnswerEvent::TurnDone {
                conversation: id,
                turn,
                truncated: stop == StopReason::MaxTokens,
            },
            Err(EngineError::Cancelled) => return,
            Err(e) => {
                log::warn!("answer failed: {e}");
                failure(&friendly(&e), needs_setup(&e))
            }
        };
        self.sink.send(event);
    }
}

async fn toolbox_run(
    toolbox: &Arc<dyn Toolbox>,
    name: &str,
    input: &serde_json::Value,
    cancel: &CancellationToken,
) -> Result<Result<String, String>, EngineError> {
    tokio::select! {
        _ = cancel.cancelled() => Err(EngineError::Cancelled),
        result = toolbox.run(name, input) => Ok(result),
    }
}

fn needs_setup(error: &EngineError) -> bool {
    matches!(
        error,
        EngineError::NotConfigured | EngineError::InvalidKey | EngineError::NoCredits
    )
}

/// What the popover says when a turn fails. Never names a vendor.
fn friendly(error: &EngineError) -> String {
    match error {
        EngineError::NotConfigured => "Set up an AI engine in Settings to get answers.".into(),
        EngineError::InvalidKey => "The API key was rejected. Check it in Settings.".into(),
        EngineError::NoCredits => {
            "The account is out of credits. Add credits, or use another key in Settings.".into()
        }
        EngineError::RateLimited { .. } => {
            "Too many requests right now. Try again in a moment.".into()
        }
        EngineError::Overloaded => "The AI service is busy. Try again in a moment.".into(),
        EngineError::Offline(_) => "Couldn't reach the AI service. Check your connection.".into(),
        EngineError::Api { message, .. } => format!("The AI service returned an error: {message}"),
        EngineError::Protocol(_) => "Something went wrong reading the answer. Try again.".into(),
        EngineError::Cancelled => "Cancelled.".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::context::{Capture, Classifier};
    use crate::engines::{EngineInfo, Limits, Readiness, Role};
    use crate::platform::{Inspection, Point, Rect, Screenshots};

    /// An engine that replays scripted outcomes and records every request.
    struct FakeEngine {
        script: Mutex<VecDeque<Result<(Message, StopReason), EngineError>>>,
        seen: Mutex<Vec<(Effort, Vec<Message>, usize)>>,
    }

    impl FakeEngine {
        fn new(script: Vec<Result<(Message, StopReason), EngineError>>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(script.into()),
                seen: Mutex::default(),
            })
        }
    }

    fn answer(text: &str) -> Result<(Message, StopReason), EngineError> {
        Ok((
            Message {
                role: Role::Assistant,
                content: vec![Block::Text(text.into())],
            },
            StopReason::EndTurn,
        ))
    }

    fn tool_call(name: &str) -> Result<(Message, StopReason), EngineError> {
        Ok((
            Message {
                role: Role::Assistant,
                content: vec![Block::ToolUse {
                    id: format!("call-{name}"),
                    name: name.into(),
                    input: serde_json::json!({}),
                }],
            },
            StopReason::ToolUse,
        ))
    }

    #[async_trait]
    impl Engine for FakeEngine {
        fn info(&self) -> EngineInfo {
            EngineInfo {
                id: "fake",
                name: "Fake",
                key_label: "",
                key_help: "",
                key_help_url: "",
            }
        }

        fn limits(&self, _effort: Effort) -> Limits {
            Limits { tool_budget: 2 }
        }

        async fn check_ready(&self) -> Readiness {
            Readiness::Ready
        }

        async fn ask(
            &self,
            req: AskRequest,
            events: mpsc::UnboundedSender<EngineEvent>,
            _cancel: CancellationToken,
        ) -> Result<crate::engines::AskOutcome, EngineError> {
            self.seen
                .lock()
                .unwrap()
                .push((req.effort, req.messages.clone(), req.tools.len()));
            let (message, stop) = self.script.lock().unwrap().pop_front().expect("scripted")?;
            let _ = events.send(EngineEvent::TextDelta(message.text()));
            Ok(crate::engines::AskOutcome {
                message,
                stop,
                usage: Usage::default(),
            })
        }
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<AnswerEvent>>);

    impl AnswerSink for Recorder {
        fn send(&self, event: AnswerEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    impl Recorder {
        fn events(&self) -> Vec<AnswerEvent> {
            self.0.lock().unwrap().clone()
        }
    }

    struct Echo;

    #[async_trait]
    impl Toolbox for Echo {
        fn specs(&self) -> Vec<ToolSpec> {
            vec![ToolSpec {
                name: "echo".into(),
                description: "Echoes".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }]
        }

        async fn run(&self, name: &str, _input: &serde_json::Value) -> Result<String, String> {
            Ok(format!("{name} ran"))
        }
    }

    fn pack() -> ContextPack {
        ContextPack::build(
            Capture {
                cursor: Point::default(),
                lens: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                window: None,
                inspection: Inspection::default(),
                focus_level: 0,
                screenshots: Screenshots::default(),
            },
            &Classifier::builtin(),
        )
    }

    fn setup(
        script: Vec<Result<(Message, StopReason), EngineError>>,
    ) -> (
        Arc<Orchestrator>,
        Arc<FakeEngine>,
        Arc<Recorder>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let settings = Arc::new(SettingsStore::load(dir.path().join("settings.json")));
        let engine = FakeEngine::new(script);
        let recorder = Arc::new(Recorder::default());
        let orchestrator = Arc::new(Orchestrator::new(
            engine.clone(),
            settings,
            recorder.clone(),
        ));
        (orchestrator, engine, recorder, dir)
    }

    fn history_roles(o: &Orchestrator) -> Vec<Role> {
        o.state()
            .current
            .as_ref()
            .unwrap()
            .history
            .iter()
            .map(|m| m.role)
            .collect()
    }

    #[tokio::test]
    async fn the_first_answer_is_low_effort_and_go_deeper_steps_up() {
        let (o, engine, recorder, _dir) =
            setup(vec![answer("short"), answer("longer"), answer("longest")]);
        o.begin(pack(), false).unwrap().await.unwrap();
        o.go_deeper().unwrap().await.unwrap();
        o.go_deeper().unwrap().await.unwrap();
        assert!(o.go_deeper().is_none(), "High is the default ceiling");

        let efforts: Vec<Effort> = engine.seen.lock().unwrap().iter().map(|s| s.0).collect();
        assert_eq!(efforts, [Effort::Low, Effort::Medium, Effort::High]);
        // Append-only: each request carries all earlier turns.
        let lengths: Vec<usize> = engine
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.1.len())
            .collect();
        assert_eq!(lengths, [1, 3, 5]);
        assert!(recorder.events().contains(&AnswerEvent::TurnDone {
            conversation: 1,
            turn: 3,
            truncated: false
        }));
    }

    #[tokio::test]
    async fn ask_mode_waits_for_the_question() {
        let (o, engine, recorder, _dir) = setup(vec![answer("It's waterproof.")]);
        assert!(o.begin(pack(), true).is_none());
        assert!(engine.seen.lock().unwrap().is_empty());
        assert!(matches!(
            recorder.events()[0],
            AnswerEvent::Started {
                awaiting_question: true,
                ..
            }
        ));

        o.ask("Is it waterproof?".into()).unwrap().await.unwrap();
        let seen = engine.seen.lock().unwrap();
        assert!(seen[0].1[0].text().ends_with("Question: Is it waterproof?"));
    }

    #[tokio::test]
    async fn tools_run_within_their_budget() {
        let (o, engine, _recorder, _dir) = setup(vec![
            tool_call("echo"),
            tool_call("echo"),
            tool_call("echo"),
            answer("done"),
        ]);
        let handle = o.begin_with(Arc::new(pack()), Arc::new(Echo), false, None);
        handle.unwrap().await.unwrap();

        let seen = engine.seen.lock().unwrap();
        // Tools are offered while the budget (2) lasts, then withheld.
        let offered: Vec<usize> = seen.iter().map(|s| s.2).collect();
        assert_eq!(offered, [1, 1, 0, 0]);
        let last = &seen[3].1;
        let results: Vec<(&String, bool)> = last
            .iter()
            .flat_map(|m| &m.content)
            .filter_map(|b| match b {
                Block::ToolResult {
                    content, is_error, ..
                } => Some((content, *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], (&"echo ran".to_string(), false));
        assert!(results[2].1, "the third call is over budget");
    }

    #[tokio::test]
    async fn a_failed_turn_leaves_no_trace() {
        let (o, _engine, recorder, _dir) =
            setup(vec![answer("first"), Err(EngineError::Overloaded)]);
        o.begin(pack(), false).unwrap().await.unwrap();
        o.ask("more?".into()).unwrap().await.unwrap();
        assert_eq!(history_roles(&o), [Role::User, Role::Assistant]);
        assert!(recorder.events().iter().any(|e| matches!(
            e,
            AnswerEvent::Failed {
                turn: 2,
                needs_setup: false,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn setup_problems_point_to_settings() {
        let (o, _engine, recorder, _dir) = setup(vec![Err(EngineError::InvalidKey)]);
        o.begin(pack(), false).unwrap().await.unwrap();
        assert!(recorder.events().iter().any(|e| matches!(
            e,
            AnswerEvent::Failed {
                needs_setup: true,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn correcting_the_target_starts_over_with_the_correction() {
        let (o, engine, recorder, _dir) = setup(vec![answer("a speaker"), answer("the price")]);
        o.begin(pack(), false).unwrap().await.unwrap();
        o.correct_target("the price tag".into())
            .unwrap()
            .await
            .unwrap();

        let seen = engine.seen.lock().unwrap();
        assert_eq!(seen[1].1.len(), 1, "a fresh conversation");
        assert!(
            seen[1].1[0]
                .text()
                .contains("The user says they pointed at: the price tag")
        );
        assert!(recorder.events().contains(&AnswerEvent::Started {
            conversation: 2,
            ceiling: Effort::High,
            awaiting_question: false
        }));
    }

    #[tokio::test]
    async fn text_streams_to_the_popover() {
        let (o, _engine, recorder, _dir) = setup(vec![answer("TARGET: x\n\nHello")]);
        o.begin(pack(), false).unwrap().await.unwrap();
        assert!(recorder.events().contains(&AnswerEvent::Delta {
            conversation: 1,
            turn: 1,
            text: "TARGET: x\n\nHello".into()
        }));
    }
}
