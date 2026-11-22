use super::*;

/// A control that shrinks while pressed, and a highlight behind it that mirrors its press scale.
fn view(_: &(), _: &mut Cx<()>) -> El {
    col()
        .size(200.0, 200.0)
        .child(el().key("b").absolute(20.0, 20.0).size(100.0, 100.0).fill(Color::WHITE).press_scale(0.9).on_press_tx(|_: &mut (), _: &mut Tx| {}))
        .child(el().key("hl").absolute(20.0, 20.0).size(100.0, 100.0).fill(Color::WHITE).press_scale_of("b").inert())
}

fn scale_of(rt: &Runtime<()>, key: &str) -> f32 {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap().scale
}

#[test]
fn a_highlight_follows_the_press_scale_of_the_control_it_names() {
    let mut rt = Runtime::new((), view, (200.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.pointer_move(70.0, 70.0);
    rt.frame(1.0 / 120.0);
    rt.pointer_down(70.0, 70.0);
    for _ in 0..30 { rt.frame(1.0 / 120.0); }
    assert_eq!(scale_of(&rt, "b"), 0.9, "the control shrinks while held");
    assert_eq!(scale_of(&rt, "hl"), 0.9, "the highlight shrinks with it");
    rt.pointer_up(70.0, 70.0);
    for _ in 0..60 { rt.frame(1.0 / 120.0); }
    assert!((scale_of(&rt, "b") - 1.0).abs() < 1e-3);
    assert!((scale_of(&rt, "hl") - 1.0).abs() < 1e-3, "the highlight springs back with it");
}
