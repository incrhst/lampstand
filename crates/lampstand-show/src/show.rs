//! Live state: what is in preview, what is live, and the mode.

use crate::library::Library;
use crate::protocol::{Command, CueView, ItemView, Mode, Pos, Snapshot};
use crate::service::{expand, Expanded, Service};

pub struct Show {
    service: Service,
    expanded: Vec<Expanded>,
    preview: Option<Pos>,
    live: Option<Pos>,
    mode: Mode,
    service_started_at: Option<u64>,
    item_started_at: Option<u64>,
    revision: u64,
}

impl Show {
    pub fn new(service: Service, lib: &Library) -> Show {
        let mut show = Show {
            service: Service::default(),
            expanded: Vec::new(),
            preview: None,
            live: None,
            mode: Mode::Normal,
            service_started_at: None,
            item_started_at: None,
            revision: 0,
        };
        show.set_service(service, lib);
        show
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    /// Replace the service order (an edit from the operator). Preview and live follow
    /// their items by id. If the live item was removed, nothing is live, so the room
    /// never shows something that is no longer in the order.
    pub fn set_service(&mut self, service: Service, lib: &Library) {
        let old_ids: Vec<String> = self.service.items.iter().map(|i| i.id.clone()).collect();
        let follow = |p: Option<Pos>, new: &Service, expanded: &[Expanded]| -> Option<Pos> {
            let p = p?;
            let id = old_ids.get(p.item)?;
            let item = new.items.iter().position(|i| &i.id == id)?;
            let cues = expanded[item].cues.len();
            (cues > 0).then(|| Pos { item, cue: p.cue.min(cues - 1) })
        };
        let expanded: Vec<Expanded> = service.items.iter().map(|i| expand(i, lib)).collect();
        self.preview = follow(self.preview, &service, &expanded);
        self.live = follow(self.live, &service, &expanded);
        self.service = service;
        self.expanded = expanded;
        if self.preview.is_none() {
            self.preview = self.first_cue_from(0);
        }
        self.revision += 1;
    }

    /// Expand every item again, e.g. after an import made a missing song or deck available.
    pub fn refresh(&mut self, lib: &Library) {
        let s = self.service.clone();
        self.set_service(s, lib);
    }

    pub fn apply(&mut self, command: &Command, now_ms: u64) {
        let before_live = self.live;
        match command {
            Command::GoLive => {
                if let Some(p) = self.preview {
                    self.live = Some(p);
                    self.preview = self.step(p, 1).or(Some(p));
                }
            }
            Command::Next => match self.live {
                Some(p) => {
                    if let Some(n) = self.step(p, 1) {
                        self.live = Some(n);
                        self.preview = self.step(n, 1).or(Some(n));
                    }
                }
                None => return self.apply(&Command::GoLive, now_ms),
            },
            Command::Prev => {
                if let Some(p) = self.live.and_then(|p| self.step(p, -1)) {
                    self.live = Some(p);
                    self.preview = self.step(p, 1).or(Some(p));
                }
            }
            Command::Select { pos } => {
                if self.valid(*pos) {
                    self.preview = Some(*pos);
                }
            }
            Command::GoTo { pos } => {
                if self.valid(*pos) {
                    self.live = Some(*pos);
                    self.preview = self.step(*pos, 1).or(Some(*pos));
                }
            }
            Command::Mode { mode } => {
                self.mode = if self.mode == *mode { Mode::Normal } else { *mode };
            }
            Command::ClearLive => self.live = None,
        }
        if self.live.is_some() && self.live != before_live {
            self.service_started_at.get_or_insert(now_ms);
            if before_live.map(|p| p.item) != self.live.map(|p| p.item) {
                self.item_started_at = Some(now_ms);
            }
            // Taking something live from black or logo is the operator saying "show it".
            if matches!(self.mode, Mode::Black | Mode::Logo) && matches!(command, Command::GoLive | Command::GoTo { .. }) {
                self.mode = Mode::Normal;
            }
        }
        if self.live.is_none() {
            self.item_started_at = None;
        }
        self.revision += 1;
    }

    fn valid(&self, p: Pos) -> bool {
        self.expanded.get(p.item).is_some_and(|e| p.cue < e.cues.len())
    }

    fn first_cue_from(&self, item: usize) -> Option<Pos> {
        (item..self.expanded.len()).find(|&i| !self.expanded[i].cues.is_empty()).map(|item| Pos { item, cue: 0 })
    }

    fn last_cue_before(&self, item: usize) -> Option<Pos> {
        (0..item).rev().find(|&i| !self.expanded[i].cues.is_empty()).map(|i| Pos { item: i, cue: self.expanded[i].cues.len() - 1 })
    }

    /// One cue forward or back through the whole service, skipping empty items.
    fn step(&self, p: Pos, dir: i32) -> Option<Pos> {
        let n = self.expanded.get(p.item)?.cues.len();
        if dir > 0 {
            if p.cue + 1 < n {
                Some(Pos { item: p.item, cue: p.cue + 1 })
            } else {
                self.first_cue_from(p.item + 1)
            }
        } else if p.cue > 0 {
            Some(Pos { item: p.item, cue: p.cue - 1 })
        } else {
            self.last_cue_before(p.item)
        }
    }

    fn cue(&self, p: Option<Pos>) -> Option<CueView> {
        let p = p?;
        self.expanded.get(p.item)?.cues.get(p.cue).cloned()
    }

    pub fn snapshot(&self, logo: Option<String>) -> Snapshot {
        let next = match self.live {
            Some(p) => self.step(p, 1),
            None => self.preview,
        };
        Snapshot {
            service_name: self.service.name.clone(),
            items: self
                .service
                .items
                .iter()
                .zip(&self.expanded)
                .map(|(item, e)| ItemView {
                    id: item.id.clone(),
                    kind: e.kind,
                    title: e.title.clone(),
                    cues: e.cues.iter().map(|c| c.label().to_string()).collect(),
                    problem: e.problem.clone(),
                })
                .collect(),
            preview: self.preview,
            live: self.live,
            mode: self.live_mode(),
            preview_cue: self.cue(self.preview),
            live_cue: self.cue(self.live),
            next_cue: self.cue(next),
            logo,
            service_started_at: self.service_started_at,
            item_started_at: self.item_started_at,
            revision: self.revision,
        }
    }

    /// A logo item live means the logo is showing, whatever the mode says.
    fn live_mode(&self) -> Mode {
        match self.live {
            Some(p) if self.mode == Mode::Normal && self.expanded[p.item].kind == crate::protocol::ItemKind::Logo => Mode::Logo,
            _ => self.mode,
        }
    }
}
