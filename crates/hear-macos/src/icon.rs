use anyhow::{Context, Result};
use tray_icon::Icon;

// Match the Linux tray's 22×22 microphone geometry, with antialiasing for Retina displays.
const SIZE: u32 = 44;
const CRADLE: &[[f32; 2]] = &[
    [5.0, 8.0],
    [5.0, 10.0],
    [7.0, 13.0],
    [9.0, 15.0],
    [13.0, 15.0],
    [15.0, 13.0],
    [17.0, 10.0],
    [17.0, 8.0],
];
const STAND: &[[f32; 2]] = &[[11.0, 15.0], [11.0, 19.0], [8.0, 19.0], [14.0, 19.0]];

pub fn microphone(color: [u8; 3]) -> Result<Icon> {
    Icon::from_rgba(pixels(color), SIZE, SIZE).context("could not create the menu-bar icon")
}

fn pixels(color: [u8; 3]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut coverage = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.0) / 2.0;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.0) / 2.0;
                    let capsule =
                        (px - 11.0).powi(2) + (py - py.clamp(5.0, 12.0)).powi(2) <= 3.0_f32.powi(2);
                    if capsule || stroke([px, py], CRADLE) || stroke([px, py], STAND) {
                        coverage += 1;
                    }
                }
            }
            rgba.extend_from_slice(&[color[0], color[1], color[2], (coverage * 255 / 16) as u8]);
        }
    }
    rgba
}

fn stroke(point: [f32; 2], path: &[[f32; 2]]) -> bool {
    path.windows(2).any(|segment| {
        let [a, b] = [segment[0], segment[1]];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let t = (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / (dx * dx + dy * dy))
            .clamp(0.0, 1.0);
        (point[0] - a[0] - t * dx).powi(2) + (point[1] - a[1] - t * dy).powi(2) <= 1.0
    })
}
