//! Serial worker: one thread owns the port and exchanges messages with the UI.
//!
//! Keeping the port off the UI thread means a slow read, an unplug or a retry
//! never blocks a frame; the UI only ever calls `try_recv`/`send`.

use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use himos_core::dm850g::connect;
use himos_core::transport::SerialLink;
use himos_core::{Frame, Session};

/// Commands the UI sends to the serial worker.
#[derive(Debug)]
pub(crate) enum ToWorker {
    /// Write one frame to the device.
    Send(Frame),
    /// Stop the worker.
    Shutdown,
}

/// Events the worker reports back to the UI.
#[derive(Debug)]
pub(crate) enum FromWorker {
    /// Port opened and handshake sent.
    Connected(String),
    /// A frame arrived from the device.
    Frame(Frame),
    /// Transport or protocol error.
    Error(String),
    /// Worker stopped.
    Closed,
}

/// Opens the port, streams frames, and reconnects until shutdown is requested.
pub(crate) fn run_worker(
    port: &str,
    commands: &Receiver<ToWorker>,
    events: &Sender<FromWorker>,
) -> Result<(), String> {
    let mut open_failures = 0u8;
    loop {
        match SerialLink::open_retry(port) {
            Ok(link) => {
                open_failures = 0;
                let mut session = Session::new(link);
                session
                    .send(&connect())
                    .map_err(|err| format!("handshake: {err}"))?;
                let _ = events.send(FromWorker::Connected(port.to_owned()));

                match pump(&mut session, commands, events) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        // A dropped link is recoverable: report and reconnect.
                        let _ = events.send(FromWorker::Error(format!("link lost: {err}")));
                        if wait_for_shutdown(commands, Duration::from_millis(500)) {
                            return Ok(());
                        }
                    }
                }
            }
            Err(err) => {
                open_failures += 1;
                let _ = events.send(FromWorker::Error(format!("open {port}: {err}")));
                if open_failures >= 5 {
                    return Err(format!(
                        "giving up on {port} after {open_failures} attempts"
                    ));
                }
                if wait_for_shutdown(commands, Duration::from_millis(500)) {
                    return Ok(());
                }
            }
        }
    }
}

/// Writes queued frames and forwards received frames; `Ok(())` means shutdown.
fn pump(
    session: &mut Session<SerialLink>,
    commands: &Receiver<ToWorker>,
    events: &Sender<FromWorker>,
) -> io::Result<()> {
    let mut buf = [0u8; 256];
    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                ToWorker::Send(frame) => session.send(&frame)?,
                ToWorker::Shutdown => return Ok(()),
            }
        }
        for frame in session.poll(&mut buf)? {
            let _ = events.send(FromWorker::Frame(frame));
        }
    }
}

/// Sleeps for `delay`, returning true when a shutdown arrives meanwhile.
fn wait_for_shutdown(commands: &Receiver<ToWorker>, delay: Duration) -> bool {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(ToWorker::Shutdown) => return true,
            Ok(ToWorker::Send(_)) | Err(_) => {}
        }
    }
    false
}
