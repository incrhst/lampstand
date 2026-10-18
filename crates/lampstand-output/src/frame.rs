//! What a snapshot puts on a projector.
//!
//! [`Frame::from_snapshot`] is the whole policy (docs/v0.1.md, "Show model"):
//!
//! | Mode | Projector |
//! | --- | --- |
//! | `normal` | the live cue; black when nothing is live |
//! | `clear` | the background only |
//! | `black` | black |
//! | `logo` | the logo fitted into the safe area; black without a logo |

use std::sync::Arc;

use lampstand_render::{Background, Color, Content, Image, Slide, TextSlide};
use lampstand_show::protocol::{CueView, Mode, Snapshot};

/// The one v0.1 theme's background, behind text cues and in `clear` mode.
pub const THEME_BACKGROUND: Color = Color::rgb(14, 22, 48);

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Black,
    /// The theme background with nothing on it.
    Background,
    /// `body` holds one `lines` entry per `\n`: each is a paragraph (a verse,
    /// a lyric line) that wraps inside the safe area if it is too long.
    Text { body: String, footer: Option<String> },
    /// A deck step, fitted into the safe area on black.
    Image { url: String },
    /// The church logo, fitted into the safe area on black.
    Logo { url: String },
}

impl Frame {
    pub fn from_snapshot(snapshot: &Snapshot) -> Frame {
        match snapshot.mode {
            Mode::Black => Frame::Black,
            Mode::Clear => Frame::Background,
            Mode::Logo => snapshot.logo.clone().map_or(Frame::Black, |url| Frame::Logo { url }),
            Mode::Normal => match &snapshot.live_cue {
                None => Frame::Black,
                Some(CueView::Text { lines, footer, .. }) => Frame::Text { body: lines.join("\n"), footer: footer.clone() },
                Some(CueView::Image { url, .. }) => Frame::Image { url: url.clone() },
            },
        }
    }

    /// The server-relative image this frame needs, if any.
    pub fn image_url(&self) -> Option<&str> {
        match self {
            Frame::Image { url } | Frame::Logo { url } => Some(url),
            _ => None,
        }
    }

    /// The slide to draw. `image` is the decoded [`image_url`](Self::image_url);
    /// without it an image frame is black.
    pub fn slide(&self, image: Option<Arc<Image>>) -> Slide {
        match self {
            Frame::Black => Slide::default(),
            Frame::Background => Slide { background: Background::Color(THEME_BACKGROUND), content: Content::Blank },
            Frame::Text { body, footer } => {
                let text = TextSlide { body: body.clone(), footer: footer.clone().filter(|f| !f.is_empty()) };
                Slide::text(text).with_background(Background::Color(THEME_BACKGROUND))
            }
            Frame::Image { .. } | Frame::Logo { .. } => image.map(Slide::image).unwrap_or_default(),
        }
    }
}

/// Images worth fetching ahead for this snapshot: the live cue, what `next`
/// or `go_live` would show, and the logo. Going live is then one upload.
pub fn prefetch_urls(snapshot: &Snapshot) -> Vec<String> {
    let cues = [&snapshot.live_cue, &snapshot.next_cue, &snapshot.preview_cue];
    let mut urls: Vec<String> = Vec::new();
    let images = cues.into_iter().flatten().filter_map(|cue| match cue {
        CueView::Image { url, .. } => Some(url.clone()),
        CueView::Text { .. } => None,
    });
    for url in images.chain(snapshot.logo.clone()) {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// Whether a state with `revision` should replace what is shown. States can
/// arrive out of order across a reconnect; `last` is `None` right after
/// connecting, when a restarted server may have started counting again.
pub fn accept_revision(last: Option<u64>, revision: u64) -> bool {
    last.is_none_or(|last| revision >= last)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn snapshot(mode: Mode, live_cue: Option<CueView>) -> Snapshot {
        Snapshot {
            service_name: "Test".into(),
            items: Vec::new(),
            preview: None,
            live: None,
            mode,
            preview_cue: None,
            live_cue,
            next_cue: None,
            logo: None,
            service_started_at: None,
            item_started_at: None,
            revision: 1,
        }
    }

    pub fn text_cue() -> CueView {
        CueView::Text {
            lines: vec!["Amazing grace".into(), "how sweet the sound".into()],
            footer: Some("John Newton".into()),
            label: "V1".into(),
            notes: None,
        }
    }

    pub fn image_cue(url: &str) -> CueView {
        CueView::Image { url: url.into(), aspect: 16.0 / 9.0, label: "Slide 1".into(), notes: None }
    }

    #[test]
    fn normal_shows_the_live_cue() {
        let s = snapshot(Mode::Normal, Some(text_cue()));
        assert_eq!(
            Frame::from_snapshot(&s),
            Frame::Text { body: "Amazing grace\nhow sweet the sound".into(), footer: Some("John Newton".into()) }
        );
        let s = snapshot(Mode::Normal, Some(image_cue("/media/deck/a/0/0.png")));
        assert_eq!(Frame::from_snapshot(&s), Frame::Image { url: "/media/deck/a/0/0.png".into() });
        assert_eq!(Frame::from_snapshot(&snapshot(Mode::Normal, None)), Frame::Black);
    }

    #[test]
    fn modes_override_the_cue() {
        let mut s = snapshot(Mode::Black, Some(text_cue()));
        assert_eq!(Frame::from_snapshot(&s), Frame::Black);
        s.mode = Mode::Clear;
        assert_eq!(Frame::from_snapshot(&s), Frame::Background);
        s.mode = Mode::Logo;
        assert_eq!(Frame::from_snapshot(&s), Frame::Black, "no logo set");
        s.logo = Some("/media/logo.png".into());
        assert_eq!(Frame::from_snapshot(&s), Frame::Logo { url: "/media/logo.png".into() });
    }

    #[test]
    fn slides() {
        assert_eq!(Frame::Black.slide(None), Slide::default());
        assert_eq!(Frame::Background.slide(None).background, Background::Color(THEME_BACKGROUND));
        let text = Frame::Text { body: "a\nb".into(), footer: Some(String::new()) }.slide(None);
        assert_eq!(text.content, Content::Text(TextSlide::new("a\nb")), "empty footer dropped");
        let img = Arc::new(Image::new(1, 1, vec![255, 0, 0, 255]));
        let frame = Frame::Image { url: "/x.png".into() };
        assert_eq!(frame.slide(None), Slide::default(), "black until the image arrives");
        match frame.slide(Some(img.clone())).content {
            Content::Image(i) => assert!(Arc::ptr_eq(&i, &img), "same Arc, so the GPU copy is reused"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn prefetches_live_next_preview_and_logo() {
        let mut s = snapshot(Mode::Normal, Some(text_cue()));
        s.next_cue = Some(image_cue("/n.png"));
        s.preview_cue = Some(image_cue("/n.png"));
        s.logo = Some("/media/logo.png".into());
        assert_eq!(prefetch_urls(&s), vec!["/n.png".to_string(), "/media/logo.png".into()]);
    }

    #[test]
    fn drops_older_revisions() {
        assert!(accept_revision(None, 0));
        assert!(accept_revision(Some(5), 5));
        assert!(accept_revision(Some(5), 6));
        assert!(!accept_revision(Some(5), 4));
    }
}
