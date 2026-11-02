//! Single-command HCI adapter. Response bytes are owned, never pointers into
//! another future's return buffer. Cancelled commands drain before reuse.

use bt_hci::cmd::{self, CmdReturnBuf};
use bt_hci::controller::{Controller, ControllerCmdAsync, ControllerCmdSync};
use bt_hci::event::{CommandComplete, CommandCompleteWithStatus, CommandStatus, EventKind};
use bt_hci::param::{RemainingBytes, Status};
use bt_hci::transport::Transport;
use bt_hci::{data, ControllerToHostPacket, FixedSizeValue, FromHciBytes, FromHciBytesError};
use core::cell::Cell;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embedded_io::ErrorType;

struct Response {
    status: Status,
    length: usize,
    bytes: [u8; 255],
}

pub struct SerialController<T> {
    transport: T,
    command: Mutex<NoopRawMutex, ()>,
    pending: Cell<Option<cmd::Opcode>>,
    credits: Cell<usize>,
    credit_changed: Signal<NoopRawMutex, ()>,
    response: Signal<NoopRawMutex, Response>,
}

impl<T> SerialController<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            command: Mutex::new(()),
            pending: Cell::new(None),
            credits: Cell::new(1),
            credit_changed: Signal::new(),
            response: Signal::new(),
        }
    }

    fn complete(&self, opcode: cmd::Opcode, status: Status, credits: usize, data: &[u8]) {
        self.credits.set(credits);
        self.credit_changed.signal(());
        if self.pending.get() != Some(opcode) {
            log::warn!("Ignoring unsolicited HCI command response {opcode:?}");
            return;
        }
        let mut response = Response {
            status,
            length: data.len(),
            bytes: [0; 255],
        };
        if data.len() > response.bytes.len() {
            response.status = bt_hci::param::Error::INVALID_HCI_PARAMETERS.to_status();
            response.length = 0;
            log::error!("HCI command response exceeds packet limits");
        } else {
            response.bytes[..data.len()].copy_from_slice(data);
        }
        self.response.signal(response);
    }
}

impl<T: ErrorType> ErrorType for SerialController<T> {
    type Error = T::Error;
}

impl<T: Transport> SerialController<T> {
    async fn execute<C: cmd::Cmd>(&self, command: &C) -> Result<Response, cmd::Error<T::Error>> {
        let _guard = self.command.lock().await;
        if self.pending.get().is_some() {
            // A dropped exec still has a controller response in flight.
            self.response.wait().await;
            self.pending.set(None);
        }
        while self.credits.get() == 0 {
            self.credit_changed.wait().await;
        }
        self.response.reset();
        self.pending.set(Some(C::OPCODE));
        self.credits.set(self.credits.get() - 1);
        if let Err(err) = self.transport.write(command).await {
            self.pending.set(None);
            // A transport failure may have sent part of the command. Do not
            // manufacture a credit; the host must tear down this connection.
            return Err(cmd::Error::Io(err));
        }
        let response = self.response.wait().await;
        self.pending.set(None);
        Ok(response)
    }
}

impl<T: Transport> Controller for SerialController<T>
where
    T::Error: From<FromHciBytesError>,
{
    async fn write_acl_data(&self, packet: &data::AclPacket<'_>) -> Result<(), Self::Error> {
        self.transport.write(packet).await
    }

    async fn write_sync_data(&self, packet: &data::SyncPacket<'_>) -> Result<(), Self::Error> {
        self.transport.write(packet).await
    }

    async fn write_iso_data(&self, packet: &data::IsoPacket<'_>) -> Result<(), Self::Error> {
        self.transport.write(packet).await
    }

    async fn read<'a>(
        &self,
        buffer: &'a mut [u8],
    ) -> Result<ControllerToHostPacket<'a>, Self::Error> {
        loop {
            // A consumed command event is not returned, so its buffer can be
            // reused on the next iteration (same contract as bt-hci's adapter).
            let bytes =
                unsafe { core::slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len()) };
            let packet = self.transport.read(bytes).await?;
            if let ControllerToHostPacket::Event(event) = &packet {
                match event.kind {
                    EventKind::CommandComplete => {
                        let complete = CommandComplete::from_hci_bytes_complete(event.data)?;
                        if !complete.has_status() {
                            self.credits.set(usize::from(complete.num_hci_cmd_pkts));
                            self.credit_changed.signal(());
                            return Ok(packet);
                        }
                        let complete: CommandCompleteWithStatus = complete.try_into()?;
                        self.complete(
                            complete.cmd_opcode,
                            complete.status,
                            usize::from(complete.num_hci_cmd_pkts),
                            complete.return_param_bytes.as_ref(),
                        );
                        continue;
                    }
                    EventKind::CommandStatus => {
                        let status = CommandStatus::from_hci_bytes_complete(event.data)?;
                        self.complete(
                            status.cmd_opcode,
                            status.status,
                            usize::from(status.num_hci_cmd_pkts),
                            &[],
                        );
                        continue;
                    }
                    _ => {}
                }
            }
            return Ok(packet);
        }
    }
}

impl<T, C> ControllerCmdSync<C> for SerialController<T>
where
    T: Transport,
    T::Error: From<FromHciBytesError>,
    C: cmd::SyncCmd,
    C::Return: FixedSizeValue,
{
    async fn exec(&self, command: &C) -> Result<C::Return, cmd::Error<Self::Error>> {
        let response = self.execute(command).await?;
        let mut buffer = C::ReturnBuf::new();
        response.status.to_result()?;
        if response.length != buffer.as_ref().len() {
            log::error!("HCI response length does not match command");
            return Err(cmd::Error::Hci(
                bt_hci::param::Error::INVALID_HCI_PARAMETERS,
            ));
        }
        buffer
            .as_mut()
            .copy_from_slice(&response.bytes[..response.length]);
        let return_param_bytes = RemainingBytes::from_hci_bytes_complete(buffer.as_ref())
            .map_err(|_| cmd::Error::Hci(bt_hci::param::Error::INVALID_HCI_PARAMETERS))?;
        CommandCompleteWithStatus {
            num_hci_cmd_pkts: 0,
            status: response.status,
            cmd_opcode: C::OPCODE,
            return_param_bytes,
        }
        .to_result::<C>()
        .map_err(cmd::Error::Hci)
    }
}

impl<T, C> ControllerCmdAsync<C> for SerialController<T>
where
    T: Transport,
    T::Error: From<FromHciBytesError>,
    C: cmd::AsyncCmd,
{
    async fn exec(&self, command: &C) -> Result<(), cmd::Error<Self::Error>> {
        self.execute(command)
            .await?
            .status
            .to_result()
            .map_err(cmd::Error::Hci)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_hci::cmd::{
        controller_baseband::Reset, le::LeReadLocalSupportedFeatures,
        link_control::ReadRemoteVersionInformation, Cmd,
    };
    use bt_hci::AsHciBytes;
    use core::future::{poll_fn, Future};
    use core::pin::Pin;
    use core::task::{Context, Poll, Waker};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct Error;

    impl core::fmt::Display for Error {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "transport error")
        }
    }
    impl core::error::Error for Error {}
    impl embedded_io::Error for Error {
        fn kind(&self) -> embedded_io::ErrorKind {
            embedded_io::ErrorKind::Other
        }
    }
    impl From<FromHciBytesError> for Error {
        fn from(_: FromHciBytesError) -> Self {
            Self
        }
    }

    #[derive(Default)]
    struct Mock {
        writes: Cell<usize>,
        failure: Cell<bool>,
        packets: RefCell<VecDeque<std::vec::Vec<u8>>>,
    }

    impl ErrorType for Mock {
        type Error = Error;
    }
    impl Transport for Mock {
        async fn write<P: bt_hci::HostToControllerPacket>(&self, _: &P) -> Result<(), Error> {
            self.writes.set(self.writes.get() + 1);
            if self.failure.get() {
                Err(Error)
            } else {
                Ok(())
            }
        }
        async fn read<'a>(&self, bytes: &'a mut [u8]) -> Result<ControllerToHostPacket<'a>, Error> {
            let packet = poll_fn(|_| match self.packets.borrow_mut().pop_front() {
                Some(packet) => Poll::Ready(packet),
                None => Poll::Pending,
            })
            .await;
            bytes[..packet.len()].copy_from_slice(&packet);
            Ok(ControllerToHostPacket::from_hci_bytes_complete(
                &bytes[..packet.len()],
            )?)
        }
    }

    fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    async fn reset(controller: &SerialController<Mock>) -> Result<(), cmd::Error<Error>> {
        ControllerCmdSync::exec(controller, &Reset::new()).await
    }

    async fn features(
        controller: &SerialController<Mock>,
    ) -> Result<bt_hci::param::LeFeatureMask, cmd::Error<Error>> {
        ControllerCmdSync::exec(controller, &LeReadLocalSupportedFeatures::new()).await
    }

    #[test]
    fn concurrent_commands_serialize_and_own_return_bytes() {
        let controller = SerialController::new(Mock::default());
        let mut first = std::boxed::Box::pin(reset(&controller));
        let mut second = std::boxed::Box::pin(features(&controller));
        assert!(poll(first.as_mut()).is_pending());
        assert!(poll(second.as_mut()).is_pending());
        assert_eq!(controller.transport.writes.get(), 1);
        controller.complete(Reset::OPCODE, Status::SUCCESS, 1, &[]);
        assert!(matches!(poll(first.as_mut()), Poll::Ready(Ok(()))));
        assert!(poll(second.as_mut()).is_pending());
        let mut bytes = [0; 8];
        controller.complete(
            LeReadLocalSupportedFeatures::OPCODE,
            Status::SUCCESS,
            1,
            &bytes,
        );
        bytes.fill(0xff);
        match poll(second.as_mut()) {
            Poll::Ready(Ok(mask)) => assert_eq!(mask.as_hci_bytes(), &[0; 8]),
            _ => panic!("Feature response did not complete"),
        }
        assert_eq!(controller.transport.writes.get(), 2);
    }

    #[test]
    fn cancelled_command_drains_late_response_before_same_opcode_reuse() {
        let controller = SerialController::new(Mock::default());
        let mut first = std::boxed::Box::pin(reset(&controller));
        assert!(poll(first.as_mut()).is_pending());
        drop(first);
        let mut second = std::boxed::Box::pin(reset(&controller));
        assert!(poll(second.as_mut()).is_pending());
        assert_eq!(controller.transport.writes.get(), 1);
        controller.complete(Reset::OPCODE, Status::SUCCESS, 1, &[]);
        assert!(poll(second.as_mut()).is_pending());
        assert_eq!(controller.transport.writes.get(), 2);
        controller.complete(Reset::OPCODE, Status::SUCCESS, 1, &[]);
        assert!(matches!(poll(second.as_mut()), Poll::Ready(Ok(()))));
    }

    #[test]
    fn zero_command_credit_blocks_until_unsolicited_credit_event() {
        let controller = SerialController::new(Mock::default());
        let mut first = std::boxed::Box::pin(reset(&controller));
        assert!(poll(first.as_mut()).is_pending());
        controller.complete(Reset::OPCODE, Status::SUCCESS, 0, &[]);
        assert!(matches!(poll(first.as_mut()), Poll::Ready(Ok(()))));
        let mut second = std::boxed::Box::pin(reset(&controller));
        assert!(poll(second.as_mut()).is_pending());
        assert_eq!(controller.transport.writes.get(), 1);
        controller.complete(cmd::Opcode::UNSOLICITED, Status::SUCCESS, 1, &[]);
        assert!(poll(second.as_mut()).is_pending());
        assert_eq!(controller.transport.writes.get(), 2);
    }

    #[test]
    fn command_errors_and_invalid_response_lengths_surface() {
        let controller = SerialController::new(Mock::default());
        let mut command = std::boxed::Box::pin(reset(&controller));
        assert!(poll(command.as_mut()).is_pending());
        controller.complete(Reset::OPCODE, Status::SUCCESS, 1, &[1]);
        assert!(matches!(
            poll(command.as_mut()),
            Poll::Ready(Err(cmd::Error::Hci(_)))
        ));
        let mut command = std::boxed::Box::pin(reset(&controller));
        assert!(poll(command.as_mut()).is_pending());
        controller.complete(
            Reset::OPCODE,
            bt_hci::param::Error::CONTROLLER_BUSY.to_status(),
            1,
            &[],
        );
        assert!(matches!(
            poll(command.as_mut()),
            Poll::Ready(Err(cmd::Error::Hci(_)))
        ));
        controller.transport.failure.set(true);
        let mut command = std::boxed::Box::pin(reset(&controller));
        assert!(matches!(
            poll(command.as_mut()),
            Poll::Ready(Err(cmd::Error::Io(_)))
        ));
        assert_eq!(controller.pending.get(), None);
        assert_eq!(controller.credits.get(), 0);
    }

    #[test]
    fn packet_reader_consumes_status_and_preserves_ordinary_events() {
        let controller = SerialController::new(Mock::default());
        let command = ReadRemoteVersionInformation::new(bt_hci::param::ConnHandle::new(1));
        let mut command = std::boxed::Box::pin(ControllerCmdAsync::exec(&controller, &command));
        assert!(poll(command.as_mut()).is_pending());
        let opcode = ReadRemoteVersionInformation::OPCODE.to_raw().to_le_bytes();
        controller
            .transport
            .packets
            .borrow_mut()
            .push_back(std::vec![4, 0x0f, 4, 0, 1, opcode[0], opcode[1]]);
        controller
            .transport
            .packets
            .borrow_mut()
            .push_back(std::vec![4, 0xff, 1, 42]);
        let mut bytes = [0; 255];
        let mut read = std::boxed::Box::pin(controller.read(&mut bytes));
        assert!(matches!(
            poll(read.as_mut()),
            Poll::Ready(Ok(ControllerToHostPacket::Event(_)))
        ));
        assert!(matches!(poll(command.as_mut()), Poll::Ready(Ok(()))));
    }

    #[test]
    fn repeated_boot_commands_do_not_leak_slots_or_borrows() {
        let controller = SerialController::new(Mock::default());
        for _ in 0..1000 {
            let mut command = std::boxed::Box::pin(reset(&controller));
            assert!(poll(command.as_mut()).is_pending());
            controller.complete(Reset::OPCODE, Status::SUCCESS, 1, &[]);
            assert!(matches!(poll(command.as_mut()), Poll::Ready(Ok(()))));
            assert_eq!(controller.pending.get(), None);
        }
    }
}
