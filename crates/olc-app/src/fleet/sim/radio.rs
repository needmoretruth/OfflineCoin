//! A simulated radio: whatever one board says, the others on the channel hear.
use olc_node::{Link, Target};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Boxes = Arc<Mutex<Vec<VecDeque<(u32, Vec<u8>)>>>>;

#[derive(Default)]
pub struct Radio {
    boxes: Boxes,
}

pub struct RadioLink {
    boxes: Boxes,
    me: usize,
}

impl Radio {
    pub fn join(&self) -> RadioLink {
        let me = self.boxes.lock().map_or(0, |mut boxes| {
            boxes.push(VecDeque::new());
            boxes.len() - 1
        });
        RadioLink {
            boxes: self.boxes.clone(),
            me,
        }
    }
}

impl Link for RadioLink {
    fn send(&mut self, target: Target, message: &[u8]) {
        let Ok(mut boxes) = self.boxes.lock() else {
            return;
        };
        for (index, slot) in boxes.iter_mut().enumerate() {
            let wanted = match target {
                Target::Peer(one) => index as u32 == one,
                Target::All => true,
                Target::AllExcept(other) => index as u32 != other,
            };
            if wanted && index != self.me {
                slot.push_back((self.me as u32, message.to_vec()));
            }
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.boxes.lock().ok()?.get_mut(self.me)?.pop_front()
    }
}
