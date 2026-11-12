//! Undo/redo with checkpoints.
use eggs::{Egg, History};

#[derive(Egg, Debug)]
struct Editor {
    text: String,
    cursor: usize,
}

fn main() {
    let mut editor = Editor { text: String::new(), cursor: 0 };
    let mut history = History::new(&editor);

    for word in ["hello", " ", "world"] {
        editor.text.push_str(word);
        editor.cursor = editor.text.len();
        history.record(&editor);
    }
    history.checkpoint("typed");

    history.undo(&mut editor).unwrap();
    history.undo(&mut editor).unwrap();
    println!("after two undos: {editor:?}");

    history.redo(&mut editor).unwrap();
    println!("after redo: {editor:?}");

    history.restore_checkpoint("typed", &mut editor).unwrap();
    println!("at checkpoint: {editor:?}");
}
