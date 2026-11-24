//! What a surface renderer keeps once its window rests.

use super::*;

on_each_backend!(rest_lets_go_of_frames_no_longer_drawn, rest_on_metal, rest_lets_go_of_frames_no_longer_drawn_on);
/// A draw keeps the bind groups of frames it showed before, for a ring that may come round again;
/// at rest it lets those frames' surfaces go and keeps only the one on show.
fn rest_lets_go_of_frames_no_longer_drawn_on(gpu: Gpu) {
    let mut renderer = SurfaceRenderer::new(&gpu.device, OFFSCREEN_FORMAT);
    let surface = NativeSurface::new();
    let shown = [draw(&surface, [0.0, 0.0, 16.0, 16.0])];
    surface.push(macos::PixelBuffer::from_bytes(&synthetic(16, 16, 0)).unwrap(), 1).unwrap();
    render(&gpu, &mut renderer, &shown, (16, 16));
    let one_frame = renderer.live_planes();
    surface.push(macos::PixelBuffer::from_bytes(&synthetic(16, 16, 1)).unwrap(), 2).unwrap();
    render(&gpu, &mut renderer, &shown, (16, 16));
    assert!(renderer.live_planes() > one_frame, "the earlier frame's bind group is kept");

    renderer.rest();
    gpu.queue.submit(std::iter::empty());
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(renderer.live_planes(), one_frame, "only the frame on show stays imported");
    let p = render(&gpu, &mut renderer, &shown, (16, 16));
    assert_eq!(p[3], 255, "the frame on show still draws after a rest");
}
