
/// Lesson 21: eight opens race to create the same parent directory.
#[test]
fn lesson_21_opens_racing_to_create_the_same_parent() {
    let dir = TestDir::new();
    let mut lost = 0;
    for round in 0..20 {
        let parent = dir.0.join(format!("round-{round}")).join("shared-parent");
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let (parent, start) = (parent.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    Wal::open(parent.join(format!("node-{n}")), 1).map(|_| ())
                })
            })
            .collect();
        for worker in workers {
            if let Err(error) = worker.join().unwrap() {
                if lost == 0 {
                    println!("LESSON first refusal: {error}");
                }
                lost += 1;
            }
        }
    }
    println!("LESSON opens refused: {lost} of 160");
}
