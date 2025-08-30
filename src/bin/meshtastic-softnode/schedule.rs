use std::collections::VecDeque;

use tokio::time::Instant;

use crate::config;

pub struct Schedule {
    items: VecDeque<(Instant, (usize, usize))>,
}

impl Schedule {
    pub fn new(channels: &[config::SoftNodeChannel]) -> Self {
        let mut items = Vec::new();
        let now = Instant::now();

        for (channel_idx, channel) in channels.iter().enumerate() {
            for (publish_idx, _) in channel.publish.iter().enumerate() {
                items.push((now, (channel_idx, publish_idx)));
            }
        }

        items.sort_by_key(|(inst, _)| *inst);

        Self {
            items: VecDeque::from(items),
        }
    }

    pub fn add(&mut self, event_time: Instant, event_data: (usize, usize)) {
        let pos = self
            .items
            .binary_search_by_key(&event_time, |(t, _)| *t)
            .unwrap_or_else(|e| e);
        self.items.insert(pos, (event_time, event_data));
    }

    pub fn next_wakeup(&self) -> Option<Instant> {
        self.items.front().map(|(inst, _)| *inst)
    }

    pub fn pop_if_completed(&mut self) -> Option<(Instant, (usize, usize))> {
        let now = Instant::now();
        if let Some((event_time, _)) = self.items.front() {
            if *event_time <= now {
                return self.items.pop_front();
            }
        }
        None
    }
}
