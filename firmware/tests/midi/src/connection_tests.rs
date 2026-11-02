use crate::connection::{self, Failure, Stage};
use core::cell::Cell;
use core::future::{pending, Future};
use core::pin::pin;
use core::task::{Context, Poll, Waker};
use embassy_futures::{
    select::{select, Either},
    yield_now,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel, signal::Signal};
use embassy_time::{Duration, MockDriver};
use nano_wire::c305::{C305FragmentReassembler, FragmentStatus};
use std::sync::Mutex;

static CLOCK_LOCK: Mutex<()> = Mutex::new(());

fn poll<F: Future>(future: core::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

struct Dropped<'a>(&'a Cell<bool>);
impl Drop for Dropped<'_> {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[test]
fn configured_deadlines_are_bounded_and_distinct() {
    assert_eq!(connection::CONNECT_TIMEOUT.as_secs(), 12);
    assert_eq!(connection::SETUP_TIMEOUT.as_secs(), 8);
    assert_eq!(connection::WRITE_TIMEOUT.as_secs(), 3);
    assert_eq!(connection::LOAD_TIMEOUT.as_secs(), 45);
}

#[test]
fn stalled_write_expires_at_exact_deadline_and_is_not_retried() {
    let _lock = CLOCK_LOCK.lock().unwrap();
    let dropped = Cell::new(false);
    let attempts = Cell::new(0);
    let operation = async {
        attempts.set(attempts.get() + 1);
        let _drop = Dropped(&dropped);
        pending::<Result<(), u8>>().await
    };
    let mut write = pin!(connection::with_deadline(
        connection::WRITE_TIMEOUT,
        operation
    ));
    assert!(poll(write.as_mut()).is_pending());
    MockDriver::get().advance(Duration::from_micros(2_999_999));
    assert!(poll(write.as_mut()).is_pending());
    MockDriver::get().advance(Duration::from_micros(1));
    assert_eq!(poll(write.as_mut()), Poll::Ready(Err(Failure::Timeout)));
    assert!(dropped.get());
    assert_eq!(attempts.get(), 1);
}

#[test]
fn bluetooth_errors_keep_the_original_code() {
    let _lock = CLOCK_LOCK.lock().unwrap();
    let mut request = pin!(connection::with_deadline(
        connection::SETUP_TIMEOUT,
        async { Err::<(), _>(0x05u8) }
    ));
    assert_eq!(
        poll(request.as_mut()),
        Poll::Ready(Err(Failure::Bluetooth(0x05)))
    );
}

#[test]
fn stalled_setup_operations_expire_at_their_configured_deadlines() {
    let _lock = CLOCK_LOCK.lock().unwrap();
    for deadline in [connection::CONNECT_TIMEOUT, connection::SETUP_TIMEOUT] {
        let mut operation = pin!(connection::with_deadline(
            deadline,
            pending::<Result<(), u8>>(),
        ));
        assert!(poll(operation.as_mut()).is_pending());
        MockDriver::get().advance(deadline - Duration::from_micros(1));
        assert!(poll(operation.as_mut()).is_pending());
        MockDriver::get().advance(Duration::from_micros(1));
        assert_eq!(poll(operation.as_mut()), Poll::Ready(Err(Failure::Timeout)));
    }
}

#[test]
fn disconnect_cancels_pending_work_at_every_stage() {
    let _lock = CLOCK_LOCK.lock().unwrap();
    for stage in [
        Stage::Searching,
        Stage::Connecting,
        Stage::Discovering,
        Stage::Subscribing,
        Stage::Loading,
        Stage::Ready,
    ] {
        let dropped = Cell::new(false);
        let restart = Signal::<NoopRawMutex, ()>::new();
        {
            let operation = async {
                let _drop = Dropped(&dropped);
                pending::<Result<(), u8>>().await
            };
            let mut session = pin!(select(
                connection::with_deadline(connection::SETUP_TIMEOUT, operation),
                restart.wait(),
            ));
            assert!(poll(session.as_mut()).is_pending(), "{stage:?}");
            restart.signal(());
            assert!(
                matches!(poll(session.as_mut()), Poll::Ready(Either::Second(()))),
                "{stage:?}"
            );
        }
        assert!(dropped.get(), "{stage:?}");
    }
}

#[test]
fn stalled_footswitch_connection_releases_radio_for_nano() {
    let _lock = CLOCK_LOCK.lock().unwrap();
    let radio = embassy_sync::mutex::Mutex::<NoopRawMutex, ()>::new(());
    let nano_acquired = Cell::new(false);
    let foot = async {
        let guard = radio.lock().await;
        let result =
            connection::with_deadline(connection::CONNECT_TIMEOUT, pending::<Result<(), u8>>())
                .await;
        drop(guard);
        assert_eq!(result, Err(Failure::Timeout));
    };
    let nano = async {
        let _guard = radio.lock().await;
        nano_acquired.set(true);
    };
    let mut links = pin!(embassy_futures::join::join(foot, nano));
    assert!(poll(links.as_mut()).is_pending());
    assert!(!nano_acquired.get());
    MockDriver::get().advance(connection::CONNECT_TIMEOUT);
    assert!(poll(links.as_mut()).is_ready());
    assert!(nano_acquired.get());
}

#[test]
fn receives_large_transfer_while_writes_wait_for_replies() {
    let _lock = CLOCK_LOCK.lock().unwrap();
    let queue = Channel::<NoopRawMutex, [u8; 512], 16>::new();
    let reply = Signal::<NoopRawMutex, ()>::new();
    let received = Cell::new(0);
    let completed = Cell::new(false);
    let receiver_dropped = Cell::new(false);
    let receiver = async {
        let _drop = Dropped(&receiver_dropped);
        let mut reassembly = C305FragmentReassembler::new();
        loop {
            let packet = queue.receive().await;
            let status = reassembly.push(&packet).unwrap();
            received.set(received.get() + 1);
            if status == FragmentStatus::Complete {
                let message = reassembly.take_message();
                assert_eq!(message.len(), 510 * 512);
                assert!(message.iter().all(|&b| b == 0x55));
                completed.set(true);
            }
            reply.signal(());
            yield_now().await;
        }
    };
    let sender = async {
        for base in (0..512).step_by(16) {
            for index in base..base + 16 {
                let mut packet = [0x55; 512];
                packet[0] = 0xf5;
                packet[1] = if index == 0 {
                    0x41
                } else if index == 511 {
                    0x81
                } else {
                    0x01
                };
                queue
                    .try_send(packet)
                    .expect("receiver must drain the bounded notification queue");
            }
            assert_eq!(queue.len(), 16);
            let result = connection::with_deadline(connection::WRITE_TIMEOUT, async {
                while received.get() < base + 16 {
                    reply.wait().await;
                }
                Ok::<_, u8>(())
            })
            .await;
            assert_eq!(result, Ok(()));
            assert_eq!(
                received.get(),
                base + 16,
                "replies must be received during the write"
            );
        }
    };
    let mut session = pin!(connection::run_io(receiver, sender));
    let mut finished = false;
    for _ in 0..2048 {
        if poll(session.as_mut()).is_ready() {
            finished = true;
            break;
        }
        MockDriver::get().advance(Duration::from_millis(1));
    }
    assert!(finished, "duplex session stalled");
    assert_eq!(received.get(), 512);
    assert!(completed.get());
    assert!(receiver_dropped.get());
}
