//! Link regions of styled text runs. A press and a release over the same `ContentLink` region of
//! one control queue a [`TextLinkEvent`]; the application drains the queue with
//! [`Runtime::take_text_link_events`]. Links do not make the Text focusable or clickable, so the
//! control keeps its passive hover, focus and `clicked` behaviour.

use super::Runtime;

/// Pending link activations kept until `take_text_link_events`; later ones are dropped and
/// counted.
pub const MAX_TEXT_LINK_EVENTS: usize = 4096;

/// Activation of a link run: `key` is the Text control's key, `link` the run's `link` id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLinkEvent {
    pub key: String,
    pub link: String,
}

#[derive(Default)]
pub(super) struct TextLinkInput {
    events: Vec<TextLinkEvent>,
    dropped: u32,
    pressed: Option<(usize, usize)>,
}

impl Runtime {
    /// Link region under a screen point of control `index`.
    fn link_at(&self, index: usize, x: f32, y: f32) -> Option<usize> {
        let control = self.controls.get(index)?;
        if control.template.links.is_empty() {
            return None;
        }
        let b = control.bounds_rect();
        let (lx, ly) = (x - b[0], y - b[1]);
        control.template.links.iter().position(|l| {
            let r = l.bounds;
            lx >= r[0] && ly >= r[1] && lx < r[0] + r[2] && ly < r[1] + r[3]
        })
    }
    /// The link id under a screen point, if the topmost reachable control has one there.
    pub fn text_link_at(&self, x: f32, y: f32) -> Option<String> {
        let hit = usize::try_from(self.hit_index(x, y)).ok()?;
        let link = self.link_at(hit, x, y)?;
        Some(self.controls[hit].template.links[link].id.clone())
    }
    pub(super) fn route_link_pointer(&mut self, x: f32, y: f32, kind: u8, hit: Option<usize>) {
        let under = hit.and_then(|i| self.link_at(i, x, y).map(|l| (i, l)));
        match kind {
            1 => self.text_links.pressed = under,
            2 => {
                if let Some((i, l)) = self.text_links.pressed.take().filter(|p| Some(*p) == under) {
                    let event = TextLinkEvent {
                        key: self.controls[i].key(),
                        link: self.controls[i].template.links[l].id.clone(),
                    };
                    if self.text_links.events.len() >= MAX_TEXT_LINK_EVENTS {
                        self.text_links.dropped = self.text_links.dropped.saturating_add(1);
                    } else {
                        self.text_links.events.push(event);
                    }
                }
            }
            3 => self.text_links.pressed = None,
            _ => {}
        }
    }
    /// Drain queued link activations, oldest first.
    pub fn take_text_link_events(&mut self) -> Vec<TextLinkEvent> {
        std::mem::take(&mut self.text_links.events)
    }
    /// Link activations dropped because the queue was full.
    pub fn text_link_events_dropped(&self) -> u32 {
        self.text_links.dropped
    }
}
