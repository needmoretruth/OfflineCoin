fn main() {
    let radius: i32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let step: usize = std::env::args().nth(2).unwrap().parse().unwrap();
    for frame in (0..olc_logo::FRAMES).step_by(step) {
        let spin = olc_logo::Spin::new(olc_logo::frame_angle(frame));
        for y in -radius..radius {
            let row: String = (-radius..radius)
                .map(|x| {
                    if olc_logo::lit(spin, x, y, radius) {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("{row}");
        }
    }
}
