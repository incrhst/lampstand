//! Print each slide's transition and animations as the DeckCraft model holds them.
fn main() {
    let path = std::env::args().nth(1).expect("file");
    let only: Option<usize> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    let p = deckcraft_pptx::import(&std::fs::read(path).expect("read")).expect("import");
    for (i, s) in p.slides.iter().enumerate() {
        if only.is_some_and(|n| n != i + 1) {
            continue;
        }
        println!(
            "slide {}: transition={:?}",
            i + 1,
            s.transition
                .as_ref()
                .map(|t| (&t.kind, &t.option, t.duration_ms, t.advance_after_ms))
        );
        for a in &s.animations {
            println!(
                "  {:?} {} opt={} start={:?} para={:?} build={:?} dur={} delay={} trig={:?} preset={:?}/{:?}",
                a.class,
                a.effect,
                a.option,
                a.start,
                a.paragraph,
                a.text_build,
                a.duration_ms,
                a.delay_ms,
                a.trigger,
                a.preset_id,
                a.preset_subtype
            );
        }
    }
}
