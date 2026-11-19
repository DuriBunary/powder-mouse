use eframe::egui::IconData;
use png::{ColorType, Transformations};
use std::io::Cursor;
use std::sync::OnceLock;

const ICON_SIZE: usize = 32;
const MOUSE_WIDTH: usize = 20;
const MOUSE_HEIGHT: usize = 27;

struct RgbaImage {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

/// Build a small app icon that shows the receiver connection and battery state.
pub fn status_icon_data(connected: bool, battery_percent: Option<u8>) -> IconData {
    let mut rgba = vec![0; ICON_SIZE * ICON_SIZE * 4];
    fill_rounded_rect(&mut rgba, 1, 1, 30, 30, 7, [239, 244, 247, 255]);

    if let Some(mouse) = mouse_photo() {
        composite_scaled(mouse, &mut rgba, 6, 2, MOUSE_WIDTH, MOUSE_HEIGHT);
    } else {
        draw_fallback_mouse(&mut rgba);
    }

    draw_connection_mark(&mut rgba, connected);
    draw_battery_gauge(&mut rgba, battery_percent);

    IconData {
        rgba,
        width: ICON_SIZE as u32,
        height: ICON_SIZE as u32,
    }
}

fn mouse_photo() -> Option<&'static RgbaImage> {
    static MOUSE: OnceLock<Option<RgbaImage>> = OnceLock::new();
    MOUSE
        .get_or_init(|| decode_mouse_photo(include_bytes!("../assets/daxa-air-mini.png")))
        .as_ref()
}

fn decode_mouse_photo(bytes: &[u8]) -> Option<RgbaImage> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        Transformations::EXPAND | Transformations::ALPHA | Transformations::STRIP_16,
    );
    let mut reader = decoder.read_info().ok()?;
    let buffer_size = reader.output_buffer_size()?;
    let mut buffer = vec![0; buffer_size];
    let info = reader.next_frame(&mut buffer).ok()?;
    let source = &buffer[..info.buffer_size()];
    let width = info.width as usize;
    let height = info.height as usize;
    let mut pixels = Vec::with_capacity(width.checked_mul(height)?.checked_mul(4)?);

    match reader.output_color_type().0 {
        ColorType::Rgba => pixels.extend_from_slice(source),
        ColorType::Rgb => {
            for rgb in source.chunks_exact(3) {
                pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        ColorType::GrayscaleAlpha => {
            for gray_alpha in source.chunks_exact(2) {
                pixels.extend_from_slice(&[
                    gray_alpha[0],
                    gray_alpha[0],
                    gray_alpha[0],
                    gray_alpha[1],
                ]);
            }
        }
        ColorType::Grayscale => {
            for gray in source {
                pixels.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
        }
        ColorType::Indexed => return None,
    }

    (pixels.len() == width.checked_mul(height)?.checked_mul(4)?).then_some(RgbaImage {
        width,
        height,
        pixels,
    })
}

fn composite_scaled(
    image: &RgbaImage,
    canvas: &mut [u8],
    left: usize,
    top: usize,
    width: usize,
    height: usize,
) {
    for dy in 0..height {
        let source_y0 = dy * image.height / height;
        let source_y1 = ((dy + 1) * image.height).div_ceil(height).min(image.height);
        for dx in 0..width {
            let source_x0 = dx * image.width / width;
            let source_x1 = ((dx + 1) * image.width).div_ceil(width).min(image.width);
            let mut alpha_sum = 0_u64;
            let mut color_alpha_sum = [0_u64; 3];
            let mut samples = 0_u64;

            for sy in source_y0..source_y1 {
                for sx in source_x0..source_x1 {
                    let offset = (sy * image.width + sx) * 4;
                    let alpha = u64::from(image.pixels[offset + 3]);
                    alpha_sum += alpha;
                    for (channel, sum) in color_alpha_sum.iter_mut().enumerate() {
                        *sum += u64::from(image.pixels[offset + channel]) * alpha;
                    }
                    samples += 1;
                }
            }

            if samples == 0 {
                continue;
            }
            let alpha = (alpha_sum / samples).min(255) as u8;
            let mut color = [0, 0, 0, alpha];
            if alpha_sum > 0 {
                for (channel, sum) in color_alpha_sum.into_iter().enumerate() {
                    color[channel] = (sum / alpha_sum).min(255) as u8;
                }
            }
            blend_pixel(canvas, left + dx, top + dy, color);
        }
    }
}

fn draw_connection_mark(canvas: &mut [u8], connected: bool) {
    draw_circle(canvas, 25, 6, 5, [255, 255, 255, 255]);
    draw_circle(
        canvas,
        25,
        6,
        3,
        if connected {
            [21, 142, 99, 255]
        } else {
            [95, 108, 119, 255]
        },
    );
    if connected {
        draw_line(canvas, 23, 6, 24, 7, [255, 255, 255, 255]);
        draw_line(canvas, 24, 7, 27, 4, [255, 255, 255, 255]);
    } else {
        draw_line(canvas, 23, 4, 27, 8, [255, 255, 255, 255]);
    }
}

fn draw_battery_gauge(canvas: &mut [u8], battery_percent: Option<u8>) {
    let left = 17;
    let top = 23;
    fill_rounded_rect(canvas, left, top, 12, 7, 1, [255, 255, 255, 255]);
    draw_rect_stroke(canvas, left, top, 12, 7, [40, 53, 63, 255]);
    fill_rect(canvas, left + 12, top + 2, 2, 3, [40, 53, 63, 255]);

    let percent = battery_percent.map(|value| value.min(100));
    let fill_width = percent.map_or(0, |value| usize::from(value) * 8 / 100);
    if fill_width > 0 {
        let color = if percent.is_some_and(|value| value < 20) {
            [220, 113, 53, 255]
        } else {
            [21, 142, 99, 255]
        };
        fill_rect(canvas, left + 2, top + 2, fill_width, 3, color);
    }
}

fn draw_fallback_mouse(canvas: &mut [u8]) {
    fill_rounded_rect(canvas, 8, 3, 16, 26, 7, [39, 48, 57, 255]);
    fill_rounded_rect(canvas, 15, 4, 2, 8, 1, [91, 219, 230, 255]);
    draw_line(canvas, 16, 12, 16, 19, [200, 210, 216, 255]);
}

fn draw_rect_stroke(
    canvas: &mut [u8],
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    color: [u8; 4],
) {
    for x in left..left + width {
        set_pixel(canvas, x, top, color);
        set_pixel(canvas, x, top + height - 1, color);
    }
    for y in top..top + height {
        set_pixel(canvas, left, y, color);
        set_pixel(canvas, left + width - 1, y, color);
    }
}

fn fill_rounded_rect(
    canvas: &mut [u8],
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    radius: usize,
    color: [u8; 4],
) {
    let radius = radius.min(width / 2).min(height / 2);
    for y in top..top + height {
        for x in left..left + width {
            let nearest_x = x.clamp(left + radius, left + width - radius - 1);
            let nearest_y = y.clamp(top + radius, top + height - radius - 1);
            let dx = x.abs_diff(nearest_x);
            let dy = y.abs_diff(nearest_y);
            if dx * dx + dy * dy <= radius * radius {
                set_pixel(canvas, x, y, color);
            }
        }
    }
}

fn fill_rect(
    canvas: &mut [u8],
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    color: [u8; 4],
) {
    for y in top..top + height {
        for x in left..left + width {
            set_pixel(canvas, x, y, color);
        }
    }
}

fn draw_circle(canvas: &mut [u8], center_x: usize, center_y: usize, radius: usize, color: [u8; 4]) {
    for y in center_y - radius..=center_y + radius {
        for x in center_x - radius..=center_x + radius {
            let dx = x.abs_diff(center_x);
            let dy = y.abs_diff(center_y);
            if dx * dx + dy * dy <= radius * radius {
                set_pixel(canvas, x, y, color);
            }
        }
    }
}

fn draw_line(canvas: &mut [u8], x0: usize, y0: usize, x1: usize, y1: usize, color: [u8; 4]) {
    let mut x = x0 as isize;
    let mut y = y0 as isize;
    let x1 = x1 as isize;
    let y1 = y1 as isize;
    let dx = (x1 - x).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let dy = -(y1 - y).abs();
    let sy = if y < y1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        set_pixel(canvas, x as usize, y as usize, color);
        if x == x1 && y == y1 {
            break;
        }
        let doubled_error = 2 * error;
        if doubled_error >= dy {
            error += dy;
            x += sx;
        }
        if doubled_error <= dx {
            error += dx;
            y += sy;
        }
    }
}

fn blend_pixel(canvas: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
    let offset = (y * ICON_SIZE + x) * 4;
    let alpha = u16::from(color[3]);
    for channel in 0..3 {
        let foreground = u16::from(color[channel]);
        let background = u16::from(canvas[offset + channel]);
        canvas[offset + channel] =
            ((foreground * alpha + background * (255 - alpha) + 127) / 255) as u8;
    }
    canvas[offset + 3] = 255;
}

fn set_pixel(canvas: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
    if x >= ICON_SIZE || y >= ICON_SIZE {
        return;
    }
    let offset = (y * ICON_SIZE + x) * 4;
    canvas[offset..offset + 4].copy_from_slice(&color);
}
