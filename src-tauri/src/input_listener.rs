use rdev::{grab, Button, Event, EventType, Key};
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

const HOLD_THRESHOLD_MS: u64 = 350;

#[derive(Debug, Clone)]
pub enum InputEvent {
    Click,
    StartSkill,
    StopSkill,
    MouseMove,
    DictationFinalizeWindowElapsed {
        session_id: u64,
    },
    DictationAsrFinished {
        session_id: u64,
        result: Result<String, String>,
    },
}

#[inline]
fn pack_position(x: f64, y: f64) -> i64 {
    let x_i32 = x as i32;
    let y_i32 = y as i32;
    ((x_i32 as i64) << 32) | (y_i32 as u32 as i64)
}

#[inline]
fn unpack_position(packed: i64) -> (f64, f64) {
    let x = (packed >> 32) as i32 as f64;
    let y = packed as i32 as f64;
    (x, y)
}

pub struct InputListener {
    pub enable_mouse: Arc<AtomicBool>,
    pub enable_alt: Arc<AtomicBool>,
    pub track_mouse_position: Arc<AtomicBool>,
    pub last_mouse_position: Arc<AtomicI64>,
}

impl InputListener {
    pub fn new() -> Self {
        Self {
            enable_mouse: Arc::new(AtomicBool::new(true)),
            enable_alt: Arc::new(AtomicBool::new(true)),
            track_mouse_position: Arc::new(AtomicBool::new(false)),
            last_mouse_position: Arc::new(AtomicI64::new(0)),
        }
    }

    pub fn get_last_mouse_position(&self) -> (f64, f64) {
        unpack_position(self.last_mouse_position.load(Ordering::Relaxed))
    }

    pub fn start(&self, tx: Sender<InputEvent>) {
        let enable_mouse = self.enable_mouse.clone();
        let enable_alt = self.enable_alt.clone();
        let track_mouse_position = self.track_mouse_position.clone();
        let last_mouse_position = self.last_mouse_position.clone();
        let middle_trigger = HoldTrigger::new();
        let alt_trigger = HoldTrigger::new();

        thread::spawn(move || {
            if let Err(error) = grab(move |event: Event| {
                let mut swallow_event = false;

                match event.event_type {
                    EventType::ButtonPress(Button::Middle) => {
                        if enable_mouse.load(Ordering::Relaxed) {
                            middle_trigger.on_press(&tx);
                            swallow_event = true;
                        }
                    }
                    EventType::ButtonRelease(Button::Middle) => {
                        if enable_mouse.load(Ordering::Relaxed) {
                            middle_trigger.on_release(&tx);
                            swallow_event = true;
                        }
                    }
                    EventType::KeyPress(Key::AltGr) => {
                        if enable_alt.load(Ordering::Relaxed) {
                            alt_trigger.on_press(&tx);
                            swallow_event = true;
                        }
                    }
                    EventType::KeyRelease(Key::AltGr) => {
                        if enable_alt.load(Ordering::Relaxed) {
                            alt_trigger.on_release(&tx);
                            swallow_event = true;
                        }
                    }
                    EventType::MouseMove { x, y } => {
                        last_mouse_position.store(pack_position(x, y), Ordering::Relaxed);

                        if track_mouse_position.load(Ordering::Relaxed) {
                            tx.send(InputEvent::MouseMove).ok();
                        }
                    }
                    _ => {}
                }

                if swallow_event {
                    None
                } else {
                    Some(event)
                }
            }) {
                println!("Error in input listener: {:?}", error);
            }
        });
    }
}

#[derive(Clone)]
struct HoldTrigger {
    pressed: Arc<AtomicBool>,
    skill_active: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

impl HoldTrigger {
    fn new() -> Self {
        Self {
            pressed: Arc::new(AtomicBool::new(false)),
            skill_active: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn on_press(&self, tx: &Sender<InputEvent>) {
        if self.pressed.swap(true, Ordering::AcqRel) {
            return;
        }

        self.skill_active.store(false, Ordering::Release);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let pressed = self.pressed.clone();
        let skill_active = self.skill_active.clone();
        let generation_state = self.generation.clone();
        let tx = tx.clone();

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(HOLD_THRESHOLD_MS));

            if generation_state.load(Ordering::Acquire) != generation {
                return;
            }

            if !pressed.load(Ordering::Acquire) {
                return;
            }

            if !skill_active.swap(true, Ordering::AcqRel) {
                tx.send(InputEvent::StartSkill).ok();
            }
        });
    }

    fn on_release(&self, tx: &Sender<InputEvent>) {
        if !self.pressed.swap(false, Ordering::AcqRel) {
            return;
        }

        if self.skill_active.swap(false, Ordering::AcqRel) {
            tx.send(InputEvent::StopSkill).ok();
        } else {
            tx.send(InputEvent::Click).ok();
        }
    }
}
