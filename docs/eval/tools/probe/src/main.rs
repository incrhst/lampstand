// Walk each deck one click at a time with deckcraft-anim's ShowState and print,
// per slide, how many clicks it took before the show moved on.
use deckcraft_anim::{ShowAction, ShowState};

fn main() {
    for path in std::env::args().skip(1) {
        let name = std::path::Path::new(&path).file_stem().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let pres = match deckcraft_pptx::import(&bytes) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut s = ShowState::new(&pres);
        let mut clicks: Vec<(usize, usize)> = vec![(s.slide, 0)];
        for _ in 0..10_000 {
            match s.next(&pres) {
                ShowAction::PlayStep(_) => {
                    clicks.last_mut().unwrap().1 += 1;
                    // Let the step's clock run out, as the UI would; auto steps chain on.
                    while let ShowAction::FinishStep(_) = s.step_done(&pres) {
                        let tl = ShowState::timeline(&pres, s.slide);
                        if s.step < tl.steps() && tl.step_is_auto(s.step) {
                            s.next(&pres);
                        } else {
                            break;
                        }
                    }
                }
                ShowAction::Slide(i) => {
                    // An auto-start first step is already playing on entry; let it finish.
                    s.step_done(&pres);
                    clicks.push((i, 0));
                }
                ShowAction::End | ShowAction::Exit => break,
                _ => {}
            }
        }
        for (i, n) in clicks {
            println!("{name}\t{i}\t{}\t{n}", ShowState::steps(&pres, i));
        }
    }
}
