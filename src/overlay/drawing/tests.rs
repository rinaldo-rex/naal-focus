use super::{FrameBuffer, SlotPool};
use smithay_client_toolkit::shm::Shm;
use std::os::unix::net::UnixStream;
use wayland_client::{
    Connection, delegate_noop,
    protocol::{wl_registry, wl_shm},
};

struct Client;
delegate_noop!(Client: ignore wl_registry::WlRegistry);
delegate_noop!(Client: ignore wl_shm::WlShm);

struct TestPool {
    pool: SlotPool,
    _connection: Connection,
    _peer: UnixStream,
}

impl TestPool {
    fn new() -> Self {
        // Allocation and activation are client-side; no compositor or protocol dispatch is needed.
        let (socket, peer) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(socket).unwrap();
        let queue = connection.new_event_queue::<Client>();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let shm: wl_shm::WlShm = registry.bind(1, 1, &queue.handle(), ());
        let pool = SlotPool::new(4096, &Shm::from(shm)).unwrap();
        Self {
            pool,
            _connection: connection,
            _peer: peer,
        }
    }

    fn frame(&mut self, width: i32, height: i32) -> FrameBuffer {
        let (buffer, _) = self
            .pool
            .create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
            .unwrap();
        FrameBuffer::new(buffer)
    }
}

fn state(frame: &FrameBuffer) -> (u64, bool, [u8; 4]) {
    (
        frame.chart_version,
        frame.entrance_dirty,
        frame.entrance_backdrop,
    )
}

#[test]
fn render_state_stays_with_its_buffer_through_breathing_and_reflection() {
    let mut fixture = TestPool::new();
    let mut frames = [fixture.frame(2, 2), fixture.frame(2, 2)];
    for frame in &frames {
        assert_eq!(state(frame), (0, false, [0; 4]));
    }
    fixture.pool.canvas(&frames[0].buffer).unwrap().fill(0x11);
    fixture.pool.canvas(&frames[1].buffer).unwrap().fill(0x22);

    frames[0].rendered(7, true, [1, 2, 3, 4]);
    frames[1].rendered(7, false, [4, 3, 2, 1]);
    assert_eq!(state(&frames[0]), (0, true, [1, 2, 3, 4]));
    assert_eq!(state(&frames[1]), (7, false, [4, 3, 2, 1]));

    frames.swap(0, 1);
    assert_eq!(state(&frames[0]), (7, false, [4, 3, 2, 1]));
    assert_eq!(state(&frames[1]), (0, true, [1, 2, 3, 4]));
    assert_eq!(fixture.pool.canvas(&frames[0].buffer).unwrap(), &[0x22; 16]);
    assert_eq!(fixture.pool.canvas(&frames[1].buffer).unwrap(), &[0x11; 16]);

    frames[1].rendered(8, false, [5, 6, 7, 8]);
    assert_eq!(state(&frames[1]), (8, false, [5, 6, 7, 8]));
    assert_eq!(state(&frames[0]), (7, false, [4, 3, 2, 1]));
}

#[test]
fn reusable_buffer_selection_skips_busy_frames_and_prioritizes_restoration() {
    let mut fixture = TestPool::new();
    let mut frames = [fixture.frame(2, 2), fixture.frame(2, 2)];
    for entrance_active in [false, true] {
        for dirty_mask in 0_u8..4 {
            for busy_mask in 0_u8..4 {
                for (i, frame) in frames.iter_mut().enumerate() {
                    frame.rendered(7, dirty_mask & (1 << i) != 0, [i as u8; 4]);
                    if busy_mask & (1 << i) != 0 {
                        frame.buffer.activate().unwrap();
                    }
                }
                let cached = [state(&frames[0]), state(&frames[1])];
                let free = |i: usize| busy_mask & (1_u8 << i) == 0;
                let restore = if entrance_active {
                    None
                } else {
                    (0..2).find(|&i| free(i) && dirty_mask & (1 << i) != 0)
                };
                let expected = restore.or_else(|| (0..2).find(|&i| free(i)));
                assert_eq!(
                    FrameBuffer::available(&frames, &mut fixture.pool, entrance_active),
                    expected,
                    "entrance={entrance_active}, dirty={dirty_mask}, busy={busy_mask}"
                );
                assert_eq!([state(&frames[0]), state(&frames[1])], cached);
                for (i, frame) in frames.iter().enumerate() {
                    if busy_mask & (1 << i) != 0 {
                        frame.buffer.deactivate().unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn replacing_buffers_resets_their_cache_state() {
    let mut fixture = TestPool::new();
    let mut frames = Vec::new();
    for (width, height) in [(16, 16), (32, 16), (16, 32)] {
        frames.clear();
        assert_eq!(
            FrameBuffer::available(&frames, &mut fixture.pool, false),
            None
        );
        for _ in 0..2 {
            let frame = fixture.frame(width, height);
            assert_eq!(state(&frame), (0, false, [0; 4]));
            assert_eq!(
                fixture.pool.canvas(&frame.buffer).unwrap().len(),
                (width * height * 4) as usize
            );
            frames.push(frame);
        }
        for frame in &mut frames {
            frame.rendered(9, true, [1, 2, 3, 4]);
        }
        assert_eq!(
            FrameBuffer::available(&frames, &mut fixture.pool, false),
            Some(0)
        );
    }
    frames.clear();
    assert_eq!(
        FrameBuffer::available(&frames, &mut fixture.pool, false),
        None
    );
    assert_eq!(fixture.pool.len(), 4096);
}
